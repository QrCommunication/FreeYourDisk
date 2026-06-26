// SPDX-License-Identifier: GPL-3.0-or-later
//! Windows desktop notifications via the WinRT `ToastNotificationManager`,
//! driven through PowerShell. No extra crate, no `unsafe`. Best-effort: any
//! failure is swallowed — a missing toast must never break a cleanup or the
//! low-space monitor.

/// Show a Windows toast with the given title and body. Best-effort (errors are
/// ignored).
pub(crate) fn show(title: &str, body: &str) {
    const PS: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";
    const APP_ID: &str =
        r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe";

    let script = format!(
        "[Windows.UI.Notifications.ToastNotificationManager,Windows.UI.Notifications,ContentType=WindowsRuntime]|Out-Null;\
         $x=[Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02);\
         $t=$x.GetElementsByTagName('text');\
         $t.Item(0).AppendChild($x.CreateTextNode('{title}'))|Out-Null;\
         $t.Item(1).AppendChild($x.CreateTextNode('{body}'))|Out-Null;\
         $n=[Windows.UI.Notifications.ToastNotification]::new($x);\
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{app}').Show($n)",
        title = escape_ps_literal(title),
        body = escape_ps_literal(body),
        app = APP_ID,
    );

    let _ = std::process::Command::new(PS)
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status();
}

/// Double single quotes so a string is safe inside a PowerShell single-quoted
/// literal (`'...'`) — the only escaping such literals require.
fn escape_ps_literal(s: &str) -> String {
    s.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::escape_ps_literal;

    #[test]
    fn doubles_single_quotes() {
        assert_eq!(escape_ps_literal("it's a 'test'"), "it''s a ''test''");
    }

    #[test]
    fn leaves_plain_text_unchanged() {
        assert_eq!(escape_ps_literal("12.3 GB freed"), "12.3 GB freed");
    }
}
