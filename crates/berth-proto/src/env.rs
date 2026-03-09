use std::collections::HashMap;

/// Parse .env file content into key-value pairs.
/// Handles comments (#), blank lines, KEY=VALUE, KEY="VALUE", KEY='VALUE', export KEY=VALUE.
pub fn parse_dotenv(content: &str) -> Vec<(String, String)> {
    let mut vars = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Strip leading "export "
        let line = line.strip_prefix("export ").unwrap_or(line);
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let mut value = value.trim().to_string();
            // Strip surrounding quotes
            if (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
            {
                value = value[1..value.len() - 1].to_string();
            }
            if !key.is_empty() {
                vars.push((key, value));
            }
        }
    }
    vars
}

/// Mask env var values in a log line. Replaces occurrences of values (length >= 3)
/// with "***". Longer values are replaced first to avoid partial matches.
pub fn mask_env_values(line: &str, env_vars: &HashMap<String, String>) -> String {
    if env_vars.is_empty() {
        return line.to_string();
    }
    let mut values: Vec<&str> = env_vars.values().map(|v| v.as_str()).collect();
    // Sort longest first
    values.sort_by(|a, b| b.len().cmp(&a.len()));
    let mut masked = line.to_string();
    for val in values {
        if val.len() >= 3 {
            masked = masked.replace(val, "***");
        }
    }
    masked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dotenv_basic() {
        let content = "KEY=value\nFOO=bar";
        let vars = parse_dotenv(content);
        assert_eq!(vars, vec![("KEY".into(), "value".into()), ("FOO".into(), "bar".into())]);
    }

    #[test]
    fn test_parse_dotenv_comments_and_blanks() {
        let content = "# comment\n\nKEY=value\n  # another comment\n";
        let vars = parse_dotenv(content);
        assert_eq!(vars, vec![("KEY".into(), "value".into())]);
    }

    #[test]
    fn test_parse_dotenv_quoted() {
        let content = "KEY=\"hello world\"\nFOO='single quoted'";
        let vars = parse_dotenv(content);
        assert_eq!(vars, vec![
            ("KEY".into(), "hello world".into()),
            ("FOO".into(), "single quoted".into()),
        ]);
    }

    #[test]
    fn test_parse_dotenv_export() {
        let content = "export API_KEY=secret123";
        let vars = parse_dotenv(content);
        assert_eq!(vars, vec![("API_KEY".into(), "secret123".into())]);
    }

    #[test]
    fn test_mask_env_values() {
        let mut env = HashMap::new();
        env.insert("API_KEY".into(), "my-secret-key".into());
        env.insert("SHORT".into(), "ab".into()); // too short, should not be masked

        let line = "Connecting with key my-secret-key and ab done";
        let masked = mask_env_values(line, &env);
        assert_eq!(masked, "Connecting with key *** and ab done");
    }

    #[test]
    fn test_mask_longest_first() {
        let mut env = HashMap::new();
        env.insert("A".into(), "secret".into());
        env.insert("B".into(), "secret-long".into());

        let line = "value is secret-long here";
        let masked = mask_env_values(line, &env);
        assert_eq!(masked, "value is *** here");
    }
}
