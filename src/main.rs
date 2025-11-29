use image::ImageReader;
use image::{DynamicImage, codecs::jpeg::JpegEncoder};
use std::path::PathBuf;
use std::process::Command;
use teloxide::prelude::*;
use teloxide::requests::Requester;
use teloxide::types::InputFile;
use tokio::task;
use tokio::time::{Duration, sleep};
use walkdir::WalkDir;

async fn generate_thumbnail(video_path: String) -> Option<InputFile> {
    task::spawn_blocking(move || {
        let temp_file = "temp_thumb.jpg";

        let success = Command::new("ffmpeg")
            .args([
                "-y",
                "-i",
                &video_path,
                "-ss",
                "00:00:02.000",
                "-vframes",
                "1",
                "-q:v",
                "2",
                temp_file,
            ])
            .status()
            .ok()
            .map_or(false, |s| s.success());

        let bytes_opt = if success {
            ImageReader::open(temp_file)
                .ok()
                .and_then(|r| r.decode().ok())
                .map(|img: DynamicImage| {
                    let resized = img.thumbnail(320, 180);
                    let mut bytes = Vec::new();
                    resized
                        .write_with_encoder(JpegEncoder::new_with_quality(&mut bytes, 90))
                        .ok();
                    bytes
                })
            // .flatten()
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

async fn get_video_metadata(video_path: String) -> (Option<u32>, Option<u32>, Option<u32>) {
    task::spawn_blocking(move || {
        let mut width: Option<u32> = None;
        let mut height: Option<u32> = None;
        let mut duration: Option<u32> = None;

        // Width & height
        if let Ok(output) = Command::new("ffprobe")
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
                    if let Ok(w) = w_str.trim().parse::<u32>() {
                        if w > 0 {
                            width = Some(w);
                        }
                    }
                }
                if let Some(h_str) = lines.next() {
                    if let Ok(h) = h_str.trim().parse::<u32>() {
                        if h > 0 {
                            height = Some(h);
                        }
                    }
                }
            }
        }

        // Duration (in seconds, rounded to nearest integer)
        if let Ok(output) = Command::new("ffprobe")
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
                        duration = Some(d_f.round() as u32);
                    }
                }
            }
        }

        (width, height, duration)
    })
    .await
    .unwrap_or((None, None, None))
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();
    log::info!("Starting file uploader bot...");

    let bot = Bot::from_env();

    teloxide::repl(bot, |bot: Bot, msg: Message| async move {
        if msg.text() == Some("/upload_all") {
            // Optional security – only you can trigger the upload
            // Change this to your Telegram user ID
            if let Some(user) = msg.from() {
                if user.id.0 != 6681565302 {
                    // <<< YOUR USER ID HERE
                    return respond(());
                }
            }

            let chat_id = msg.chat.id; // Uploads go to the chat where you sent the command

            bot.send_message(
                chat_id,
                "Scanning current directory recursively for files...",
            )
            .await
            .ok();

            let files: Vec<PathBuf> = task::spawn_blocking(|| {
                WalkDir::new(".")
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_file())
                    .map(|e| e.path().to_path_buf())
                    .collect()
            })
            .await
            .unwrap();

            bot.send_message(
                chat_id,
                format!("Found {} files. Starting upload...", files.len()),
            )
            .await
            .ok();

            for path in files {
                // let Ok(input_file) = InputFile::file(&path) else {
                //     continue;
                // };

                let input_file = InputFile::file(&path);
                let relative_str = path.display().to_string();

                let ext = path
                    .extension()
                    .and_then(|os| os.to_str())
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default();

                let is_image =
                    ["jpg", "jpeg", "png", "webp", "bmp", "tiff", "ico"].contains(&ext.as_str());
                let is_gif = ext == "gif" || ext == "gifv";
                let is_video = [
                    "mp4", "mov", "avi", "mkv", "webm", "flv", "wmv", "m4v", "mpg", "mpeg", "3gp",
                ]
                .contains(&ext.as_str());
                let is_audio = ["mp3", "wav", "ogg", "flac", "aac", "m4a", "opus", "wma"]
                    .contains(&ext.as_str());

                let send_result = if is_gif {
                    bot.send_animation(chat_id, input_file.clone())
                        .caption(&relative_str)
                        .await
                } else if is_image {
                    bot.send_photo(chat_id, input_file.clone())
                        .caption(&relative_str)
                        .await
                } else if is_video {
                    let thumbnail = generate_thumbnail(path.to_string_lossy().to_string()).await;
                    let (width, height, duration) =
                        get_video_metadata(path.to_string_lossy().to_string()).await;

                    let mut req = bot
                        .send_video(chat_id, input_file.clone())
                        .caption(&relative_str)
                        .supports_streaming(true);

                    if let Some(thumb) = thumbnail {
                        req = req.thumbnail(thumb);
                    }
                    if let Some(w) = width {
                        req = req.width(w as u32);
                    }
                    if let Some(h) = height {
                        req = req.height(h as u32);
                    }
                    if let Some(d) = duration {
                        req = req.duration(d as u32);
                    }

                    req.await
                } else if is_audio {
                    bot.send_audio(chat_id, input_file.clone())
                        .caption(&relative_str)
                        .await
                } else {
                    bot.send_document(chat_id, input_file.clone())
                        .caption(&relative_str)
                        .await
                };

                if send_result.is_err() {
                    let _ = bot
                        .send_message(chat_id, format!("Failed to upload: {}", &relative_str))
                        .await;
                }

                // Respect rate limits – ~1 message/sec is very safe for media
                sleep(Duration::from_millis(1100)).await;
            }

            bot.send_message(chat_id, "All files uploaded successfully!")
                .await
                .ok();
        }

        respond(())
    })
    .await;
}
