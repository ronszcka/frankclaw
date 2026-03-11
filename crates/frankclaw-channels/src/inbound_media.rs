pub(crate) fn infer_inbound_mime_type(
    explicit: Option<&str>,
    filename: Option<&str>,
    url: Option<&str>,
) -> String {
    let explicit = explicit
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(explicit) = explicit {
        return explicit.to_string();
    }

    infer_from_name(filename)
        .or_else(|| infer_from_url(url))
        .unwrap_or_else(|| "application/octet-stream".to_string())
}

fn infer_from_name(name: Option<&str>) -> Option<String> {
    let name = name?.trim();
    let ext = name.rsplit('.').next()?.trim().to_ascii_lowercase();
    infer_from_extension(&ext).map(str::to_string)
}

fn infer_from_url(url: Option<&str>) -> Option<String> {
    let url = url?.trim();
    let path = url
        .split('?')
        .next()
        .unwrap_or(url)
        .rsplit('/')
        .next()
        .unwrap_or(url);
    infer_from_name(Some(path))
}

fn infer_from_extension(ext: &str) -> Option<&'static str> {
    match ext {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        "mp3" => Some("audio/mpeg"),
        "m4a" => Some("audio/mp4"),
        "ogg" | "oga" => Some("audio/ogg"),
        "wav" => Some("audio/wav"),
        "flac" => Some("audio/flac"),
        "mp4" => Some("video/mp4"),
        "mov" => Some("video/quicktime"),
        "webm" => Some("video/webm"),
        "pdf" => Some("application/pdf"),
        "json" => Some("application/json"),
        "csv" => Some("text/csv"),
        "md" => Some("text/markdown"),
        "txt" | "log" => Some("text/plain"),
        "zip" => Some("application/zip"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::infer_inbound_mime_type;

    #[test]
    fn prefers_explicit_mime_type_when_present() {
        assert_eq!(
            infer_inbound_mime_type(
                Some("image/custom"),
                Some("photo.png"),
                Some("https://example.test/photo.jpg"),
            ),
            "image/custom"
        );
    }

    #[test]
    fn infers_from_filename_when_provider_omits_type() {
        assert_eq!(
            infer_inbound_mime_type(None, Some("voice-note.m4a"), None),
            "audio/mp4"
        );
    }

    #[test]
    fn infers_from_url_when_filename_is_missing() {
        assert_eq!(
            infer_inbound_mime_type(None, None, Some("https://cdn.example.test/path/report.pdf?sig=1")),
            "application/pdf"
        );
    }
}
