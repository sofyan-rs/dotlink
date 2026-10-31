//! Verification codes in a phone's notifications: copied to the clipboard by themselves.

/// Words that say a number nearby is a code (English and Indonesian).
const KEYWORDS: [&str; 8] = ["code", "otp", "verification", "verify", "passcode", "kode", "verifikasi", "sandi"];

/// The code in `text`: 4 to 8 digits (or Google's `G-123456`) when a keyword is in the text,
/// the one closest to a keyword when there are several. None otherwise.
pub fn find(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let keywords: Vec<usize> = KEYWORDS.iter().flat_map(|k| lower.match_indices(k).map(|(i, _)| i)).collect();
    if keywords.is_empty() {
        return None;
    }
    let bytes = text.as_bytes();
    let mut best: Option<(usize, String)> = None;
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let len = i - start;
        // Part of something longer: 12.50, 1,000, a phone number, an amount in Rp.
        let joined = |b: Option<&u8>| b.is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b',' | b'/' | b':'));
        let before = start.checked_sub(1).map(|j| &bytes[j]);
        let google = start >= 2 && &bytes[start - 2..start] == b"G-";
        if !(4..=8).contains(&len) || (joined(before) && !google && before != Some(&b'-')) || (joined(bytes.get(i)) && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            continue;
        }
        let code = &text[start..i];
        let distance = keywords.iter().map(|&k| k.abs_diff(start)).min().unwrap_or(usize::MAX);
        if best.as_ref().is_none_or(|(d, _)| distance < *d) {
            best = Some((distance, code.to_string()));
        }
    }
    best.map(|(_, code)| code)
}

#[cfg(test)]
mod tests {
    use super::find;

    #[test]
    fn codes() {
        assert_eq!(find("Your verification code is 482913").as_deref(), Some("482913"));
        assert_eq!(find("G-739201 is your Google verification code.").as_deref(), Some("739201"));
        assert_eq!(find("Kode OTP Anda: 1234. Jangan berikan ke siapa pun.").as_deref(), Some("1234"));
        assert_eq!(find("JANGAN BERIKAN KODE INI. Kode verifikasi: 55 66 77, atau 908172").as_deref(), Some("908172"));
        assert_eq!(find("Use code 4821 to log in. Order 20251031 ships soon").as_deref(), Some("4821"));
    }

    #[test]
    fn not_codes() {
        assert_eq!(find("Meeting at 1430 in room 12"), None);
        assert_eq!(find("Your code: 12"), None);
        assert_eq!(find("Total Rp 150.000 paid, code sent"), None);
        assert_eq!(find("Call 081234567890 for the code"), None);
    }
}
