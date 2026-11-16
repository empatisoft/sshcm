//! Interface language. English is the default; Turkish can be selected with
//! `--lang`, `SSHCM_LANG` or `sshcm config set language tr`. Both languages
//! are compiled into the binary and the system locale is never consulted.
//!
//! Command names, flags and config keys are the same in every language.

use std::sync::atomic::{AtomicU8, Ordering};

use sshcm_core::{DocError, Error, ValidationError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Tr,
}

impl Lang {
    pub fn parse(code: &str) -> Option<Lang> {
        match code.trim().to_ascii_lowercase().as_str() {
            "en" => Some(Lang::En),
            "tr" => Some(Lang::Tr),
            _ => None,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Tr => "tr",
        }
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: Lang) {
    LANG.store(lang as u8, Ordering::Relaxed);
}

pub fn current() -> Lang {
    if LANG.load(Ordering::Relaxed) == Lang::Tr as u8 {
        Lang::Tr
    } else {
        Lang::En
    }
}

/// Picks the text for the current language.
pub fn tr(en: &'static str, tr: &'static str) -> &'static str {
    match current() {
        Lang::En => en,
        Lang::Tr => tr,
    }
}

/// Like [`tr`], then replaces each `{}` with the next argument.
pub fn trf(en: &'static str, turkish: &'static str, args: &[&str]) -> String {
    let template = tr(en, turkish);
    let mut out = String::with_capacity(template.len() + 16);
    let mut args = args.iter();
    let mut rest = template;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        out.push_str(args.next().copied().unwrap_or(""));
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

pub fn validation(err: &ValidationError) -> String {
    use ValidationError::*;
    if current() == Lang::En {
        return err.to_string();
    }
    match err {
        AliasEmpty => "Alias zorunludur.".to_string(),
        AliasInvalidChar(c) if c.is_whitespace() => "Alias boşluk içeremez.".to_string(),
        AliasInvalidChar(c) => format!("Alias '{c}' içeremez."),
        AliasLeadingDash => "Alias '-' ile başlayamaz.".to_string(),
        AliasTaken(a) => format!("{a} adında bir bağlantı zaten var."),
        HostNameEmpty => "HostName zorunludur.".to_string(),
        HostNameInvalid => "HostName bir alan adı veya IP adresi olmalıdır.".to_string(),
        UserInvalid => "User boşluk içeremez.".to_string(),
        PortInvalid => "Port 1 ile 65535 arasında olmalıdır.".to_string(),
        IdentityFileMissing(p) => format!("Anahtar dosyası bulunamadı: {p}"),
        JumpEmpty => "En az bir atlama sunucusu seçin.".to_string(),
        JumpInvalid(h) => format!("Geçerli bir atlama sunucusu değil: {h}"),
        JumpSelf(a) => format!("{a} kendi atlama sunucusu olamaz."),
        JumpCycle(h) => format!("{h} zaten bu bağlantı üzerinden atlıyor."),
        ProxyAddressInvalid => "Proxy adresi adres:port biçiminde olmalıdır.".to_string(),
        ProxyCommandEmpty => "Proxy komutu boş olamaz.".to_string(),
        TagInvalid(t) => {
            format!("'{t}' geçerli bir etiket değil. Harf, rakam ve '-' kullanın.")
        }
        DescriptionMultiline => "Açıklama tek satır olmalıdır.".to_string(),
    }
}

pub fn doc_error(err: &DocError) -> String {
    if current() == Lang::En {
        return err.to_string();
    }
    match err {
        DocError::NotFound(a) => format!("{a} adında bir bağlantı yok."),
        DocError::ReadOnly(a) => {
            format!("{a} salt okunur: desenler ve dahil edilen dosyalar değiştirilemez.")
        }
    }
}

pub fn store_error(err: &Error) -> String {
    if current() == Lang::En {
        return err.to_string();
    }
    match err {
        Error::Io { path, source } => format!("{}: {source}", path.display()),
        Error::NotUtf8(path) => format!("{} geçerli UTF-8 değil.", path.display()),
        Error::ChangedOnDisk(path) => format!(
            "{} başka bir program tarafından değiştirildi. Hiçbir şey yazılmadı; yeniden deneyin.",
            path.display()
        ),
        Error::VerifyFailed { stderr } => {
            format!("ssh yeni config dosyasını kabul etmedi, önceki dosya geri yüklendi.\n{stderr}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trf_fills_placeholders_in_order() {
        assert_eq!(trf("{} of {}", "{} / {}", &["1", "2"]), "1 of 2");
        assert_eq!(trf("none", "yok", &["x"]), "none");
        assert_eq!(trf("{} and {}", "{} ve {}", &["a"]), "a and ");
    }

    #[test]
    fn lang_codes() {
        assert_eq!(Lang::parse("TR"), Some(Lang::Tr));
        assert_eq!(Lang::parse("en"), Some(Lang::En));
        assert_eq!(Lang::parse("de"), None);
        assert_eq!(Lang::Tr.code(), "tr");
    }
}
