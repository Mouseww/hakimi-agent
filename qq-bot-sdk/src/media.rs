use crate::auth::TokenManager;
use crate::error::{Error, Result};
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

const CHUNK_SIZE: usize = 4 * 1024 * 1024; // 4MB chunks for large files
const SMALL_FILE_THRESHOLD: usize = 10 * 1024 * 1024; // 10MB threshold

/// 富媒体上传客户端
#[derive(Clone)]
pub struct MediaClient {
    token_manager: Arc<TokenManager>,
    client: reqwest::Client,
    base_url: String,
}

impl MediaClient {
    pub fn new(token_manager: Arc<TokenManager>) -> Self {
        Self {
            token_manager,
            client: reqwest::Client::new(),
            base_url: "https://api.sgroup.qq.com".to_string(),
        }
    }

    pub fn with_sandbox(mut self) -> Self {
        self.base_url = "https://sandbox.api.sgroup.qq.com".to_string();
        self
    }

    async fn get_auth_header(&self) -> Result<String> {
        let token = self.token_manager.get_token().await?;
        Ok(format!("QQBot {}", token))
    }

    /// 上传图片（通用方法，自动选择上传策略）
    pub async fn upload_image<P: AsRef<Path>>(
        &self,
        path: P,
        message_type: MediaMessageType,
    ) -> Result<FileInfoResponse> {
        let metadata = tokio::fs::metadata(path.as_ref()).await?;
        let file_size = metadata.len() as usize;

        if file_size > SMALL_FILE_THRESHOLD {
            self.upload_large_file(path, message_type, MediaType::Image)
                .await
        } else {
            self.upload_small_file(path, message_type, MediaType::Image)
                .await
        }
    }

    /// 上传文件
    pub async fn upload_file<P: AsRef<Path>>(
        &self,
        path: P,
        message_type: MediaMessageType,
    ) -> Result<FileInfoResponse> {
        let metadata = tokio::fs::metadata(path.as_ref()).await?;
        let file_size = metadata.len() as usize;

        if file_size > SMALL_FILE_THRESHOLD {
            self.upload_large_file(path, message_type, MediaType::File)
                .await
        } else {
            self.upload_small_file(path, message_type, MediaType::File)
                .await
        }
    }

    /// 上传语音
    pub async fn upload_audio<P: AsRef<Path>>(
        &self,
        path: P,
        message_type: MediaMessageType,
    ) -> Result<FileInfoResponse> {
        self.upload_small_file(path, message_type, MediaType::Audio)
            .await
    }

    /// 上传视频
    pub async fn upload_video<P: AsRef<Path>>(
        &self,
        path: P,
        message_type: MediaMessageType,
    ) -> Result<FileInfoResponse> {
        let metadata = tokio::fs::metadata(path.as_ref()).await?;
        let file_size = metadata.len() as usize;

        if file_size > SMALL_FILE_THRESHOLD {
            self.upload_large_file(path, message_type, MediaType::Video)
                .await
        } else {
            self.upload_small_file(path, message_type, MediaType::Video)
                .await
        }
    }

    /// 小文件上传（一次性上传）
    async fn upload_small_file<P: AsRef<Path>>(
        &self,
        path: P,
        message_type: MediaMessageType,
        media_type: MediaType,
    ) -> Result<FileInfoResponse> {
        let path = path.as_ref();
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::Other("Invalid filename".to_string()))?
            .to_string();

        let mut file = File::open(path).await?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).await?;

        let url = self.build_upload_url(&message_type, &media_type);
        let auth = self.get_auth_header().await?;

        let part = Part::bytes(buffer)
            .file_name(filename.clone())
            .mime_str(&self.guess_mime_type(path))?;

        let form = Form::new()
            .part("file_image", part)
            .text("srv_send_msg", "false");

        let resp = self
            .client
            .post(&url)
            .header("Authorization", auth)
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp.text().await.unwrap_or_default();
            return Err(Error::Other(format!(
                "Upload failed ({}): {}",
                status, error_text
            )));
        }

        let result: FileInfoResponse = resp.json().await?;
        Ok(result)
    }

    /// 大文件分片上传
    async fn upload_large_file<P: AsRef<Path>>(
        &self,
        path: P,
        message_type: MediaMessageType,
        media_type: MediaType,
    ) -> Result<FileInfoResponse> {
        let path = path.as_ref();
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::Other("Invalid filename".to_string()))?
            .to_string();

        let metadata = tokio::fs::metadata(path).await?;
        let file_size = metadata.len() as usize;
        let total_chunks = (file_size + CHUNK_SIZE - 1) / CHUNK_SIZE;

        // 分片上传
        let mut file = File::open(path).await?;
        let mut chunk_index = 0;

        while chunk_index < total_chunks {
            let mut buffer = vec![0u8; CHUNK_SIZE];
            let n = file.read(&mut buffer).await?;
            buffer.truncate(n);

            let _is_last = chunk_index == total_chunks - 1;
            self.upload_chunk(
                &buffer,
                &filename,
                chunk_index,
                total_chunks,
                &message_type,
                &media_type,
            )
            .await?;

            chunk_index += 1;
        }

        // 完成上传，获取 file_info
        self.finalize_upload(&filename, &message_type, &media_type)
            .await
    }

    async fn upload_chunk(
        &self,
        data: &[u8],
        filename: &str,
        chunk_index: usize,
        total_chunks: usize,
        message_type: &MediaMessageType,
        media_type: &MediaType,
    ) -> Result<()> {
        let url = self.build_upload_url(message_type, media_type);
        let auth = self.get_auth_header().await?;

        let part = Part::bytes(data.to_vec())
            .file_name(filename.to_string())
            .mime_str("application/octet-stream")?;

        let form = Form::new()
            .part("file_data", part)
            .text("chunk_index", chunk_index.to_string())
            .text("total_chunks", total_chunks.to_string())
            .text("srv_send_msg", "false");

        let resp = self
            .client
            .post(&url)
            .header("Authorization", auth)
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp.text().await.unwrap_or_default();
            return Err(Error::Other(format!(
                "Chunk upload failed ({}): {}",
                status, error_text
            )));
        }

        Ok(())
    }

    async fn finalize_upload(
        &self,
        filename: &str,
        message_type: &MediaMessageType,
        media_type: &MediaType,
    ) -> Result<FileInfoResponse> {
        let url = self.build_upload_url(message_type, media_type);
        let auth = self.get_auth_header().await?;

        let form = Form::new()
            .text("filename", filename.to_string())
            .text("finalize", "true")
            .text("srv_send_msg", "false");

        let resp = self
            .client
            .post(&url)
            .header("Authorization", auth)
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp.text().await.unwrap_or_default();
            return Err(Error::Other(format!(
                "Finalize upload failed ({}): {}",
                status, error_text
            )));
        }

        let result: FileInfoResponse = resp.json().await?;
        Ok(result)
    }

    fn build_upload_url(&self, message_type: &MediaMessageType, media_type: &MediaType) -> String {
        let endpoint = match (message_type, media_type) {
            (MediaMessageType::Channel, MediaType::Image) => "/channels/upload/files",
            (MediaMessageType::C2C, MediaType::Image) => "/v2/users/upload/files",
            (MediaMessageType::Group, MediaType::Image) => "/v2/groups/upload/files",
            (MediaMessageType::Channel, MediaType::File) => "/channels/upload/files",
            (MediaMessageType::C2C, MediaType::File) => "/v2/users/upload/files",
            (MediaMessageType::Group, MediaType::File) => "/v2/groups/upload/files",
            (MediaMessageType::Channel, MediaType::Audio) => "/channels/upload/audio",
            (MediaMessageType::C2C, MediaType::Audio) => "/v2/users/upload/audio",
            (MediaMessageType::Group, MediaType::Audio) => "/v2/groups/upload/audio",
            (MediaMessageType::Channel, MediaType::Video) => "/channels/upload/video",
            (MediaMessageType::C2C, MediaType::Video) => "/v2/users/upload/video",
            (MediaMessageType::Group, MediaType::Video) => "/v2/groups/upload/video",
        };
        format!("{}{}", self.base_url, endpoint)
    }

    fn guess_mime_type(&self, path: &Path) -> String {
        match path.extension().and_then(|s| s.to_str()) {
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("png") => "image/png",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            Some("mp3") => "audio/mpeg",
            Some("wav") => "audio/wav",
            Some("ogg") => "audio/ogg",
            Some("mp4") => "video/mp4",
            Some("avi") => "video/x-msvideo",
            Some("mov") => "video/quicktime",
            _ => "application/octet-stream",
        }
        .to_string()
    }
}

/// 消息类型（用于上传）
#[derive(Debug, Clone, Copy)]
pub enum MediaMessageType {
    Channel,
    C2C,
    Group,
}

/// 媒体类型
#[derive(Debug, Clone, Copy)]
pub enum MediaType {
    Image,
    Audio,
    Video,
    File,
}

/// 文件上传响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfoResponse {
    pub file_info: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<u64>,
}

/// 附件信息（从消息中解析）
#[derive(Debug, Clone)]
pub struct ParsedAttachment {
    pub url: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub media_type: AttachmentMediaType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentMediaType {
    Image,
    Audio,
    Video,
    File,
    Unknown,
}

impl ParsedAttachment {
    pub fn from_attachment(att: &crate::model::MessageAttachment) -> Self {
        let media_type = if let Some(ct) = &att.content_type {
            if ct.starts_with("image/") {
                AttachmentMediaType::Image
            } else if ct.starts_with("audio/") {
                AttachmentMediaType::Audio
            } else if ct.starts_with("video/") {
                AttachmentMediaType::Video
            } else {
                AttachmentMediaType::File
            }
        } else {
            // 从 URL 或文件名推断
            let url_lower = att.url.to_lowercase();
            if url_lower.contains("image") {
                AttachmentMediaType::Image
            } else if url_lower.contains("audio") {
                AttachmentMediaType::Audio
            } else if url_lower.contains("video") {
                AttachmentMediaType::Video
            } else {
                AttachmentMediaType::Unknown
            }
        };

        Self {
            url: att.url.clone(),
            filename: att.filename.clone(),
            content_type: att.content_type.clone(),
            media_type,
        }
    }

    /// 下载附件
    pub async fn download(&self) -> Result<Vec<u8>> {
        let client = reqwest::Client::new();
        let resp = client.get(&self.url).send().await?;

        if !resp.status().is_success() {
            return Err(Error::Other(format!(
                "Failed to download attachment: {}",
                resp.status()
            )));
        }

        let bytes = resp.bytes().await?;
        Ok(bytes.to_vec())
    }

    /// 下载附件到文件
    pub async fn download_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let data = self.download().await?;
        tokio::fs::write(path, data).await?;
        Ok(())
    }
}
