use image::ImageReader;
use image::{DynamicImage, codecs::jpeg::JpegEncoder};
use std::path::PathBuf;
use std::process::Command;
use teloxide::prelude::*;
use teloxide::requests::Requester;
use teloxide::types::InputFile;
use tokio::task;

async fn generate_thumbnail(video_path: String) -> Option<InputFile> {
    task::spawn_blocking(move || {
        let temp_file = "temp_thumb.jpg";

        let success = Command::new("ffmpeg")
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
                    let resized = img.thumbnail(320, 320);
                    let mut bytes = Vec::new();
                    resized
                        .write_with_encoder(JpegEncoder::new_with_quality(&mut bytes, 100))
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

async fn get_caption(file_path: PathBuf) -> String {
    task::spawn_blocking(move || {
        // opens a file with the same name as the video file but with a .txt extension and returns its content as a string
        let caption_path = file_path.with_extension("txt");
        return std::fs::read_to_string(&caption_path).unwrap_or_default();
    })
    .await
    .unwrap_or(String::from(""))
}

async fn get_static_caption() -> String {
    task::spawn_blocking(move || {
        // opens a file with the same name as the video file but with a .txt extension and returns its content as a string
        let static_caption_path = PathBuf::from("static_caption.txt");
        return std::fs::read_to_string(&static_caption_path).unwrap_or_default();
    })
    .await
    .unwrap_or(String::from(""))
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    let path = PathBuf::from(&args[1]);
    log::info!("File: {:?}", &path);
    let _chat_id = std::env::var("TELEGOY_CHAT_ID").unwrap();
    let chat_id = _chat_id.clone();
    log::info!("Starting file uploader bot...");

    let bot = Bot::from_env().set_api_url(reqwest::Url::parse("http://localhost:8081").unwrap());

    let input_file = InputFile::file(&path);
    // let relative_str = path.display().to_string();

    let caption = get_caption(path.clone()).await;
    let static_caption = get_static_caption().await;

    let ext = &path
        .extension()
        .and_then(|os| os.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    let is_image = ["jpg", "jpeg", "png"].contains(&ext.as_str());
    let is_video = [
        "mp4", "mov", "avi", "mkv", "webm", "flv", "wmv", "m4v", "mpg", "mpeg", "3gp",
    ]
    .contains(&ext.as_str());

    let _ = if is_image {
        bot.send_photo(chat_id.clone(), input_file.clone())
            // .caption(relative_str.strip_prefix("./").unwrap())
            .await
    } else if is_video {
        let thumbnail = generate_thumbnail(path.display().to_string()).await;
        let (width, height, duration) = get_video_metadata(path.display().to_string()).await;

        let mut req = bot
            .send_video(chat_id.clone(), input_file.clone())
            .caption(caption + static_caption.as_str())
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
    } else {
        panic!()
    };

    // if send_result.is_err() {
    //     log::error!(
    //         "Failed to upload: {}",
    //         relative_str.strip_prefix("./").unwrap()
    //     );
    // }
}
