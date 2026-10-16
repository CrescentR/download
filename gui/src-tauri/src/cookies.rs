//! Cookie sources shared by analysis and downloads. An explicit file must not
//! trigger browser extraction: a locked Chromium database would abort first.
use std::{fs, path::Path};

pub fn append_args(args: &mut Vec<String>, browser: &str, file: &str) {
    if !file.trim().is_empty() {
        args.extend(["--cookies".into(), file.trim().into()]);
    } else if !browser.trim().is_empty() {
        args.extend(["--cookies-from-browser".into(), browser.trim().into()]);
    }
}

pub fn check_file(file: &str, language: &str) -> Result<(), String> {
    if !file.trim().is_empty() && !Path::new(file.trim()).is_file() {
        return Err(if language == "en" {
            "The configured Cookie file does not exist. Choose an existing cookies.txt or clear its path to use browser cookies."
        } else {
            "配置的 Cookie 文件不存在。请选择现有 cookies.txt，或清空文件路径以改用浏览器 Cookie。"
        }.into());
    }
    Ok(())
}

pub fn failure_hint(log: &str, language: &str) -> Option<String> {
    let lower = log.to_lowercase();
    let zh = language != "en";
    let message = if lower.contains("fresh cookies") && (lower.contains("douyin") || lower.contains("aweme")) {
        if zh { "抖音拒绝了当前会话或请求。已启用抖音接口适配；若仍失败，请用浏览器打开同一视频、完成页面验证，再导入抖音自己的 Cookie（不一定需要登录）。B 站 Cookie 无法用于抖音；也可检查网络/代理后重试。" }
        else { "Douyin rejected the current session or request. The compatibility adapter is enabled. If this persists, open the same video in a browser, complete any verification, and import fresh Douyin cookies (login may not be required). Bilibili cookies cannot authenticate Douyin. Check your network/proxy as well." }
    } else if lower.contains("could not copy") && lower.contains("cookie") {
        if zh { "浏览器 Cookie 数据库被占用，无法读取。请完全退出浏览器（含后台进程）后重试；也可在「网络与登录」导入 cookies.txt 或粘贴对应网站的 Cookie。" }
        else { "The browser cookie database is locked. Fully exit the browser, including background processes, then retry; or import cookies.txt / paste a site Cookie in Network & login." }
    } else if lower.contains("dpapi") || lower.contains("app-bound") || lower.contains("failed to decrypt") {
        if zh { "无法解密浏览器 Cookie。新版 Chromium 的加密可能不受当前引擎支持；请导入 cookies.txt、粘贴对应网站的 Cookie，或使用已登录的 Firefox。" }
        else { "Browser cookies could not be decrypted. Current Chromium encryption may be unsupported; import cookies.txt, paste a site Cookie, or use a signed-in Firefox profile." }
    } else if lower.contains("does not look like a netscape") || (lower.contains("cookie") && lower.contains("invalid netscape")) {
        if zh { "Cookie 文件格式不正确，需要 Netscape cookies.txt。Cookie 请求头请粘贴到设置中的专用输入框，不要直接当作文件路径。" }
        else { "The cookie file must use Netscape cookies.txt format. Paste a Cookie request header into its dedicated settings field rather than the file path." }
    } else { return None; };
    Some(message.into())
}

/// Accept Netscape cookies.txt or a header explicitly scoped to its selected site.
fn normalize_for_site(text: &str, site: &str) -> Result<String, String> {
    let text = text.trim_start_matches('\u{feff}').trim_start();
    if text.len() > 2 * 1024 * 1024 { return Err("Cookie 内容过长 / Cookie input is too large".into()); }
    let domain = match site { "bilibili" => ".bilibili.com", "douyin" => ".douyin.com", _ => return Err("Unsupported Cookie site".into()) };
    let mut rows = Vec::new();
    if text.starts_with("# Netscape HTTP Cookie File") || text.starts_with("# HTTP Cookie File") {
        for line in text.lines().map(|line| line.trim_end_matches('\r')).filter(|line| !line.trim().is_empty()) {
            if line.starts_with('#') && !line.starts_with("#HttpOnly_") { continue; }
            let fields: Vec<_> = line.split('\t').collect();
            if fields.len() != 7 || fields[0].is_empty() || fields[2].is_empty()
                || !matches!(fields[1], "TRUE" | "FALSE") || !matches!(fields[3], "TRUE" | "FALSE")
                || fields[4].parse::<u64>().is_err() || fields[5].is_empty() {
                return Err("Netscape Cookie 行格式不正确（需要 7 列） / Invalid Netscape cookie row".into());
            }
            rows.push(line.to_string());
        }
    } else {
        let header = text.strip_prefix("Cookie:").or_else(|| text.strip_prefix("cookie:")).unwrap_or(text).trim();
        if header.chars().any(|ch| ch.is_control()) { return Err("请只粘贴 Cookie 请求头的值 / Paste only the Cookie request header value".into()); }
        let mut session = false;
        for pair in header.split(';').map(str::trim).filter(|pair| !pair.is_empty()) {
            let (name, value) = pair.split_once('=').ok_or("Cookie 格式应为 name=value; name=value / Invalid Cookie header")?;
            let name = name.trim();
            if name.is_empty() || !name.bytes().all(|ch| ch.is_ascii_alphanumeric() || b"_-".contains(&ch)) {
                return Err("Cookie 名称无效 / Invalid cookie name".into());
            }
            let value = value.trim();
            if site == "douyin" && matches!(name, "SESSDATA" | "bili_jct") {
                return Err("这是 B 站 Cookie，请选择 B 站；抖音需要自己的 Cookie / Bilibili cookies cannot be used for Douyin".into());
            }
            session |= !value.is_empty() && if site == "bilibili" { name == "SESSDATA" } else {
                matches!(name, "ttwid" | "__ac_nonce" | "__ac_signature" | "s_v_web_id" | "sessionid" | "passport_csrf_token" | "UIFID" | "UIFID_TEMP" | "msToken")
            };
            rows.push(format!("{domain}\tTRUE\t/\tTRUE\t0\t{name}\t{value}"));
        }
        if !session { return Err(if site == "bilibili" {
            "B 站 Cookie 中缺少 SESSDATA，请登录后复制 Cookie 请求头 / Missing Bilibili SESSDATA"
        } else { "缺少抖音会话 Cookie，请在浏览器打开抖音视频、完成验证后复制 Cookie 请求头 / Missing Douyin visitor cookies" }.into()); }
    }
    if rows.is_empty() { return Err("Cookie 内容为空 / No cookies found".into()); }
    Ok(format!("# Netscape HTTP Cookie File\n{}\n", rows.join("\n")))
}

pub fn import_for_site(text: &str, dir: &Path, site: &str, existing: &str) -> Result<String, String> {
    let mut content = normalize_for_site(text, site)?;
    // Preserve cookies for other sites when adding another site's header.
    // User-supplied source files are read only; the merged jar is app-owned.
    if !existing.trim().is_empty() && Path::new(existing.trim()).is_file() {
        let previous = fs::read_to_string(existing.trim()).map_err(|_| "无法读取现有 Cookie 文件 / Could not read existing cookie file")?;
        let previous = normalize_for_site(&previous, site)?;
        let root = |line: &str| {
            let domain = line.split('\t').next().unwrap_or("").trim_start_matches("#HttpOnly_").trim_start_matches('.');
            if domain == "bilibili.com" || domain.ends_with(".bilibili.com") { "bilibili.com".to_string() }
            else if domain == "douyin.com" || domain.ends_with(".douyin.com") { "douyin.com".to_string() }
            else { domain.to_string() }
        };
        let domains: std::collections::HashSet<_> = content.lines().filter(|line| line.contains('\t')).map(root).collect();
        for line in previous.lines().filter(|line| line.contains('\t')) {
            if !domains.contains(&root(line)) { content.push_str(line); content.push('\n'); }
        }
    }
    fs::create_dir_all(dir).map_err(|_| "无法创建 Cookie 目录 / Could not create cookie directory")?;
    let path = dir.join(format!("cookies-{}.txt", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    use std::io::Write;
    let result = options.open(&path).and_then(|mut file| file.write_all(content.as_bytes()));
    if result.is_err() {
        let _ = fs::remove_file(&path);
        return Err("无法保存 Cookie 文件 / Could not save cookie file".into());
    }
    Ok(path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn normalize(text: &str) -> Result<String, String> { normalize_for_site(text, "bilibili") }
    fn import(text: &str, dir: &Path) -> Result<String, String> { import_for_site(text, dir, "bilibili", "") }
    #[test]
    fn file_bypasses_locked_browser_for_both_consumers() {
        let mut args = Vec::new();
        append_args(&mut args, "chrome", " C:/cookies.txt ");
        assert_eq!(args, ["--cookies", "C:/cookies.txt"]);
        args.clear(); append_args(&mut args, "firefox", "");
        assert_eq!(args, ["--cookies-from-browser", "firefox"]);
    }
    #[test]
    fn bili_header_preserves_encoded_session_and_equals() {
        let value = normalize("Cookie: SESSDATA=a%2Cb%3D; bili_jct=x=y").unwrap();
        assert!(value.contains(".bilibili.com\tTRUE\t/\tTRUE\t0\tSESSDATA\ta%2Cb%3D"));
        assert!(value.contains("\tbili_jct\tx=y"));
        assert!(normalize("session=unrelated-site").is_err());
        assert!(normalize("SESSDATA=secret\r\nHost: example.com").is_err());
    }
    #[test]
    fn netscape_import_preserves_http_only_and_rejects_invalid_rows() {
        let text = "# Netscape HTTP Cookie File\n#HttpOnly_.bilibili.com\tTRUE\t/\tTRUE\t0\tSESSDATA\tfixture";
        assert!(normalize(text).unwrap().contains("#HttpOnly_"));
        assert!(normalize("# Netscape HTTP Cookie File\nbad-row").is_err());
        assert!(normalize("# Netscape HTTP Cookie File").is_err());
    }
    #[test]
    fn browser_failures_are_actionable_without_echoing_secrets() {
        assert!(failure_hint("ERROR: Could not copy Chrome cookie database", "zh").unwrap().contains("被占用"));
        assert!(failure_hint("Failed to decrypt with DPAPI", "en").unwrap().contains("Firefox"));
    }

    #[test]
    fn import_creates_a_real_cookie_file_and_missing_paths_are_rejected() {
        let dir = std::env::temp_dir().join(format!("yuki-cookie-test-{}", uuid::Uuid::new_v4()));
        assert!(check_file(&dir.join("missing.txt").display().to_string(), "zh").is_err());
        let path = import("SESSDATA=test-fixture-only", &dir).unwrap();
        assert!(check_file(&path, "en").is_ok());
        assert!(fs::read_to_string(&path).unwrap().starts_with("# Netscape HTTP Cookie File\n"));
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn douyin_cookies_are_scoped_and_do_not_require_a_login() {
        let text = normalize_for_site("ttwid=visitor; __ac_nonce=nonce", "douyin").unwrap();
        assert!(text.contains(".douyin.com\tTRUE\t/\tTRUE\t0\tttwid\tvisitor"));
        assert!(!text.contains("bilibili.com"));
        assert!(normalize_for_site("SESSDATA=bili-token; ttwid=visitor", "douyin").is_err());
        assert!(normalize_for_site("ttwid=visitor", "bilibili").is_err());
        assert!(failure_hint("ERROR: [Douyin] Fresh cookies (not necessarily logged in) are needed", "zh").unwrap().contains("B 站 Cookie 无法用于抖音"));
    }

    #[test]
    fn importing_another_site_keeps_existing_site_sessions_and_empty_values() {
        let dir = std::env::temp_dir().join(format!("yuki-cookie-test-{}", uuid::Uuid::new_v4()));
        let bili = import("SESSDATA=bili-fixture", &dir).unwrap();
        let combined = import_for_site("ttwid=douyin-fixture", &dir, "douyin", &bili).unwrap();
        let content = fs::read_to_string(&combined).unwrap();
        assert!(content.contains(".bilibili.com\tTRUE\t/\tTRUE\t0\tSESSDATA\tbili-fixture"));
        assert!(content.contains(".douyin.com\tTRUE\t/\tTRUE\t0\tttwid\tdouyin-fixture"));
        let refreshed = import_for_site("ttwid=refreshed", &dir, "douyin", &combined).unwrap();
        let content = fs::read_to_string(&refreshed).unwrap();
        assert!(!content.contains("douyin-fixture"));
        assert!(content.contains("bili-fixture"));
        assert!(normalize_for_site("# Netscape HTTP Cookie File\n.douyin.com\tTRUE\t/\tTRUE\t0\tempty\t\n", "douyin").is_ok());
        for path in [bili, combined, refreshed] { fs::remove_file(path).unwrap(); }
        fs::remove_dir(dir).unwrap();
    }
}
