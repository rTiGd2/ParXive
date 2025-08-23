use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

/// Simple Fluent-based localizer with built-in resources.
pub struct FluentLoc {
    bundle: FluentBundle<FluentResource>,
}

impl FluentLoc {
    /// Create a localizer using built-in `.ftl` strings (see ../i18n).
    pub fn builtin(lang: &str) -> Self {
        // Fallback to en-GB if parsing fails.
        let langid: LanguageIdentifier = lang.parse().unwrap_or_else(|_| "en-GB".parse().unwrap());

        // You can add more languages later and select at runtime.
        let ftl_src = match lang {
            "en-GB" | "en" => include_str!("../i18n/en-GB.ftl"),
            _ => include_str!("../i18n/en-GB.ftl"),
        };

        let res =
            FluentResource::try_new(ftl_src.to_owned()).expect("invalid FTL resource (en-GB.ftl)");

        // Use the non-concurrent bundle constructor for stable.
        let mut bundle = FluentBundle::new(vec![langid]);
        bundle.add_resource(res).expect("failed to add FTL resource");
        Self { bundle }
    }

    /// Format a message by code with named args (("name","value"), ...).
    /// Returns the code itself if not found.
    pub fn msg(&self, code: &str, args: &[(&str, &str)]) -> String {
        let Some(msg) = self.bundle.get_message(code) else {
            return code.to_string();
        };
        let Some(pattern) = msg.value() else {
            return code.to_string();
        };

        let mut fa = FluentArgs::new();
        for (k, v) in args {
            fa.set(*k, FluentValue::from(*v));
        }

        let mut errs = vec![];
        let s = self.bundle.format_pattern(pattern, Some(&fa), &mut errs).to_string();

        if errs.is_empty() {
            s
        } else {
            code.to_string()
        }
    }
}

/// A no-op localizer you can use in tests.
pub struct NoopLoc;

impl NoopLoc {
    pub fn msg(&self, code: &str, _args: &[(&str, &str)]) -> String {
        code.to_string()
    }
}
