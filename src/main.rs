use clap::Parser;
use config::{Config, Environment, File};
use image::ImageReader;
use image::{DynamicImage, codecs::jpeg::JpegEncoder};
use serde::Deserialize;
use std::path::PathBuf;
use teloxide::prelude::*;
use teloxide::types::{InputFile, InputMedia, InputMediaPhoto, InputMediaVideo};
use tokio::task;

// ---------------------------
// 1. Configuration & CLI
// ---------------------------

#[derive(Debug, Deserialize)]
struct Settings {
    // Default chat_id can be loaded from env/config
    chat_id: String,
    // API URL for local bot server
    #[serde(default = "default_api_url")]
    api_url: String,
}

fn default_api_url() -> String {
    "http://localhost:8081".to_string()
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// List of file paths to upload (space separated)
    #[arg(required = true)]
    files: Vec<PathBuf>,

    /// Optional Chat ID (overrides config/env)
    #[arg(short, long)]
    chat_id: Option<String>,

    /// Optional static_caption.txt path (overrides config/env)
    #[arg(short, long)]
    static_caption_path: Option<String>,
}

// ---------------------------
// 2. Helper Functions
// ---------------------------

async fn generate_thumbnail(video_path: String) -> Option<InputFile> {
    task::spawn_blocking(move || {
        let temp_file = format!("temp_thumb_{}.jpg", uuid::Uuid::new_v4()); // Unique temp name

        let success = std::process::Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-v",
                "error",
                "-y",
                "-i",
                &video_path,
                "-ss",
                "00:00:00.000",
                "-frames:v",
                "1",
                "-update",
                "1",
                "-q:v",
                "2",
                &temp_file,
            ])
            .status()
            .ok()
            .map_or(false, |s| s.success());

        let bytes_opt = if success {
            ImageReader::open(&temp_file)
                .ok()
                .and_then(|r| r.decode().ok())
                .map(|img: DynamicImage| {
                    let resized = img.thumbnail(320, 320);
                    let mut bytes = Vec::new();
                    resized
                        .write_with_encoder(JpegEncoder::new_with_quality(&mut bytes, 100))
                        .ok();
                    bytes
                })
        } else {
            None
        };

        let _ = std::fs::remove_file(temp_file);
        bytes_opt.map(|b| InputFile::memory(b).file_name("thumb.jpg"))
    })
    .await
    .ok()
    .flatten()
}

async fn get_video_metadata(video_path: String) -> (Option<u16>, Option<u16>, Option<u16>) {
    task::spawn_blocking(move || {
        let mut width: Option<u16> = None;
        let mut height: Option<u16> = None;
        let mut duration: Option<u16> = None;

        // Width & height
        if let Ok(output) = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
                &video_path,
            ])
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                let mut lines = text.lines();
                if let Some(w_str) = lines.next() {
                    if let Ok(w) = w_str.trim().parse::<u16>() {
                        if w > 0 {
                            width = Some(w);
                        }
                    }
                }
                if let Some(h_str) = lines.next() {
                    if let Ok(h) = h_str.trim().parse::<u16>() {
                        if h > 0 {
                            height = Some(h);
                        }
                    }
                }
            }
        }

        // Duration
        if let Ok(output) = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
                &video_path,
            ])
            .output()
        {
            if output.status.success() {
                if let Ok(d_f) = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<f64>()
                {
                    if d_f > 0.0 {
                        duration = Some(d_f.round() as u16);
                    }
                }
            }
        }

        (width, height, duration)
    })
    .await
    .unwrap_or((None, None, None))
}

async fn get_caption(file_path: &PathBuf) -> String {
    let caption_path = file_path.with_extension("txt");
    tokio::fs::read_to_string(caption_path)
        .await
        .unwrap_or_default()
}

// async fn get_static_caption() -> String {
//     tokio::fs::read_to_string("static_caption.txt")
//         .await
//         .unwrap_or_default()
// }

// ---------------------------
// 3. Main Logic
// ---------------------------

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    // 1. Parse CLI Args
    let args = Cli::parse();

    // 2. Load Config (Environment variables prefixed with TELEGOY_ override defaults)
    let config_loader = Config::builder()
        .add_source(File::with_name("config").required(false)) // Optional config.toml
        .add_source(Environment::with_prefix("TELEGOY")) // e.g. TELEGOY_CHAT_ID
        .build();

    let settings: Settings = match config_loader.and_then(|c| c.try_deserialize()) {
        Ok(s) => s,
        Err(e) => {
            log::error!("Configuration error: {}", e);
            // Fallback just for safety if env vars are missing but args are present?
            // Better to panic or exit if we can't get basic settings.
            if args.chat_id.is_none() {
                panic!("Chat ID not found in Config, Env, or CLI.");
            }
            // Mock settings if only CLI is used
            Settings {
                chat_id: "".to_string(),
                api_url: default_api_url(),
            }
        }
    };

    // Determine final Chat ID (CLI arg takes precedence over Config/Env)
    let chat_id = args.chat_id.unwrap_or(settings.chat_id);
    let bot_url = reqwest::Url::parse(&settings.api_url).expect("Invalid API URL");

    log::info!("Starting uploader. Target Chat: {}", chat_id);

    let bot = Bot::from_env().set_api_url(bot_url);
    let mut input_media_group: Vec<InputMedia> = Vec::new();
    let static_cap = args
        .static_caption_path
        .unwrap_or("static_caption.txt".to_string());

    // 3. Process Files
    for path in args.files {
        log::info!("Processing file: {:?}", path);

        let ext = path
            .extension()
            .and_then(|os| os.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        let is_image = ["jpg", "jpeg", "png", "webp"].contains(&ext.as_str());
        let is_video = ["mp4", "mov", "avi", "mkv"].contains(&ext.as_str());

        let input_file = InputFile::file(&path);
        let file_caption = get_caption(&path).await;
        let full_caption = format!("{}{}", file_caption, static_cap);

        if is_image {
            let mut media = InputMediaPhoto::new(input_file);
            // Only attach caption to the first item usually, or all if you prefer
            if input_media_group.is_empty() {
                media = media.caption(full_caption.clone());
            }
            input_media_group.push(InputMedia::Photo(media));
        } else if is_video {
            let path_str = path.display().to_string();

            // Get Metadata
            let thumbnail = generate_thumbnail(path_str.clone()).await;
            let (width, height, duration) = get_video_metadata(path_str).await;

            let mut media = InputMediaVideo::new(input_file).supports_streaming(true);

            if input_media_group.is_empty() {
                media = media.caption(full_caption.clone());
            }

            if let Some(thumb) = thumbnail {
                media = media.thumbnail(thumb);
            }
            if let Some(w) = width {
                media = media.width(w);
            }
            if let Some(h) = height {
                media = media.height(h);
            }
            if let Some(d) = duration {
                media = media.duration(d);
            }

            input_media_group.push(InputMedia::Video(media));
        } else {
            log::warn!("Skipping unsupported file type: {:?}", path);
        }
    }

    if input_media_group.is_empty() {
        log::error!("No valid media found to send.");
        return;
    }

    // 4. Send Media Group
    log::info!("Sending {} media items...", input_media_group.len());
    match bot.send_media_group(chat_id, input_media_group).await {
        Ok(_) => log::info!("Successfully sent media group!"),
        Err(e) => log::error!("Failed to send media group: {:?}", e),
    }
}
