use anyhow::Result;
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Runtime (process-local) active-account overrides, keyed by provider
/// prefix ("claude", "openai", ...). Lets `/account switch <label>` take
/// effect immediately without rewriting the provider auth file.
///
/// Centralized here so every provider shares one mechanism instead of
/// duplicating a `static ACTIVE_ACCOUNT_OVERRIDE` per module.
static RUNTIME_ACTIVE_OVERRIDES: LazyLock<RwLock<HashMap<&'static str, String>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub fn set_runtime_active_override(prefix: &'static str, label: Option<String>) {
    if let Ok(mut overrides) = RUNTIME_ACTIVE_OVERRIDES.write() {
        match label {
            Some(label) => {
                overrides.insert(prefix, label);
            }
            None => {
                overrides.remove(prefix);
            }
        }
    }
}

pub fn runtime_active_override(prefix: &str) -> Option<String> {
    RUNTIME_ACTIVE_OVERRIDES
        .read()
        .ok()
        .and_then(|overrides| overrides.get(prefix).cloned())
}

pub fn canonical_account_label(prefix: &str, index: usize) -> String {
    format!("{prefix}-{index}")
}

pub fn next_account_label<'a>(prefix: &str, labels: impl IntoIterator<Item = &'a str>) -> String {
    let labels = labels.into_iter().collect::<std::collections::HashSet<_>>();
    (1..)
        .map(|index| canonical_account_label(prefix, index))
        .find(|label| !labels.contains(label.as_str()))
        .unwrap()
}

pub fn login_target_label<T, F>(
    prefix: &str,
    requested: Option<&str>,
    aliases: &HashMap<String, String>,
    active_label: Option<String>,
    accounts: &[T],
    label_of: F,
) -> String
where
    F: Fn(&T) -> &str + Copy,
{
    if let Some(requested) = requested
        .map(str::trim)
        .filter(|requested| !requested.is_empty())
    {
        let requested = resolve_label(aliases, requested);
        if accounts
            .iter()
            .any(|account| label_of(account) == requested)
        {
            return requested.to_string();
        }
        return next_account_label(
            prefix,
            accounts
                .iter()
                .map(label_of)
                .chain(aliases.keys().map(String::as_str))
                .chain(aliases.values().map(String::as_str)),
        );
    }

    active_label
        .or_else(|| {
            accounts
                .first()
                .map(|account| label_of(account).to_string())
        })
        .unwrap_or_else(|| canonical_account_label(prefix, 1))
}

pub fn active_account_label<T, F>(
    override_label: Option<String>,
    stored_active_label: Option<String>,
    accounts: &[T],
    label_of: F,
) -> Option<String>
where
    F: Fn(&T) -> &str + Copy,
{
    override_label.or(stored_active_label).or_else(|| {
        accounts
            .first()
            .map(|account| label_of(account).to_string())
    })
}

pub fn set_active_account<T, F>(
    label: &str,
    accounts: &[T],
    stored_active_label: &mut Option<String>,
    missing_message: &str,
    label_of: F,
) -> Result<()>
where
    F: Fn(&T) -> &str + Copy,
{
    if !accounts.iter().any(|account| label_of(account) == label) {
        anyhow::bail!(missing_message.replace("{}", label));
    }
    *stored_active_label = Some(label.to_string());
    Ok(())
}

pub fn upsert_account<T, FGet, FSet>(
    prefix: &str,
    accounts: &mut Vec<T>,
    stored_active_label: &mut Option<String>,
    mut account: T,
    aliases: &HashMap<String, String>,
    label_of: FGet,
    set_label: FSet,
) -> String
where
    FGet: Fn(&T) -> &str + Copy,
    FSet: Fn(&mut T, String) + Copy,
{
    let requested_label = resolve_label(aliases, label_of(&account)).to_string();
    set_label(&mut account, requested_label.clone());
    if let Some(existing) = accounts
        .iter_mut()
        .find(|existing| label_of(existing) == requested_label)
    {
        *existing = account;
        return requested_label;
    }

    let label = next_account_label(
        prefix,
        accounts
            .iter()
            .map(label_of)
            .chain(aliases.keys().map(String::as_str))
            .chain(aliases.values().map(String::as_str)),
    );
    let mut account = account;
    set_label(&mut account, label.clone());
    accounts.push(account);

    if stored_active_label.is_none() || accounts.len() == 1 {
        *stored_active_label = Some(label.clone());
    }

    label
}

// Old names remain reserved so an in-flight refresh cannot write to a different account.
pub fn resolve_label<'a>(aliases: &'a HashMap<String, String>, label: &'a str) -> &'a str {
    aliases.get(label).map(String::as_str).unwrap_or(label)
}

pub fn rename_account<T>(
    accounts: &mut [T],
    active: &mut Option<String>,
    aliases: &mut HashMap<String, String>,
    label: &str,
    new_label: &str,
    label_of: impl Fn(&T) -> &str,
    set_label: impl Fn(&mut T, String),
) -> Result<()> {
    if new_label.chars().any(char::is_control) {
        anyhow::bail!("Account name cannot contain control characters");
    }
    let new_label = new_label.trim();
    if new_label.is_empty() {
        anyhow::bail!("Account name cannot be empty");
    }
    let index = accounts
        .iter()
        .position(|a| label_of(a) == label)
        .ok_or_else(|| anyhow::anyhow!("No account with label '{}' found", label))?;
    if label == new_label {
        return Ok(());
    }
    if aliases.get(new_label).is_some_and(|target| target != label)
        || aliases.values().any(|target| target == new_label)
        || accounts.iter().any(|a| label_of(a) == new_label)
    {
        anyhow::bail!(
            "Account name '{}' is already used or reserved by a previous name",
            new_label
        );
    }
    for target in aliases.values_mut() {
        if target == label {
            *target = new_label.to_string();
        }
    }
    aliases.remove(new_label);
    aliases.insert(label.to_string(), new_label.to_string());
    set_label(&mut accounts[index], new_label.to_string());
    if active.as_deref() == Some(label) {
        *active = Some(new_label.to_string());
    }
    Ok(())
}

pub fn lock_store(prefix: &str) -> Result<std::fs::File> {
    let dir = crate::storage::jcode_dir()?;
    crate::storage::ensure_dir(&dir)?;
    let path = dir.join(format!("{prefix}-accounts.lock"));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    file.lock()?;
    Ok(file)
}

pub struct RelabelOutcome {
    pub changed: bool,
    pub canonical_override_label: Option<String>,
}

pub fn relabel_accounts<T, FGet, FSet>(
    prefix: &str,
    accounts: &mut [T],
    stored_active_label: &mut Option<String>,
    override_label: Option<String>,
    label_of: FGet,
    set_label: FSet,
) -> RelabelOutcome
where
    FGet: Fn(&T) -> &str + Copy,
    FSet: Fn(&mut T, String) + Copy,
{
    let _ = (prefix, override_label, set_label);
    let desired_active = stored_active_label
        .clone()
        .filter(|label| accounts.iter().any(|account| label_of(account) == label))
        .or_else(|| {
            accounts
                .first()
                .map(|account| label_of(account).to_string())
        });
    let changed = *stored_active_label != desired_active;
    *stored_active_label = desired_active;
    RelabelOutcome {
        changed,
        canonical_override_label: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct Account {
        label: String,
    }

    #[test]
    fn account_lock_creates_missing_home() {
        let _guard = crate::storage::lock_test_env();
        let temp = tempfile::TempDir::new().unwrap();
        let previous = std::env::var_os("JCODE_HOME");
        crate::env::set_var("JCODE_HOME", temp.path().join("new-home"));
        let result = lock_store("openai");
        if let Some(previous) = previous {
            crate::env::set_var("JCODE_HOME", previous);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn rename_back_to_own_old_name_preserves_redirects() {
        let mut accounts = vec![
            Account {
                label: "Work".into(),
            },
            Account {
                label: "Other".into(),
            },
        ];
        let mut active = Some("Other".into());
        let mut aliases = HashMap::new();
        rename_account(
            &mut accounts,
            &mut active,
            &mut aliases,
            "Work",
            "Personal",
            |a| a.label.as_str(),
            |a, label| a.label = label,
        )
        .unwrap();
        assert!(
            rename_account(
                &mut accounts,
                &mut active,
                &mut aliases,
                "Personal",
                "Other",
                |a| a.label.as_str(),
                |a, label| a.label = label
            )
            .is_err()
        );
        rename_account(
            &mut accounts,
            &mut active,
            &mut aliases,
            "Personal",
            "Work",
            |a| a.label.as_str(),
            |a, label| a.label = label,
        )
        .unwrap();
        assert_eq!(resolve_label(&aliases, "Personal"), "Work");
        assert!(!aliases.contains_key("Work"));
        assert_eq!(active.as_deref(), Some("Other"));
    }

    #[test]
    fn saved_names_survive_loading() {
        let mut accounts = vec![Account {
            label: "Work account".into(),
        }];
        let mut active = Some("Work account".into());
        let outcome = relabel_accounts(
            "openai",
            &mut accounts,
            &mut active,
            None,
            |a| a.label.as_str(),
            |a, label| a.label = label,
        );
        assert_eq!(accounts[0].label, "Work account");
        assert!(!outcome.changed);
    }

    #[test]
    fn new_numbered_name_skips_existing_collision() {
        let mut accounts = vec![Account {
            label: "openai-2".into(),
        }];
        let mut active = Some("openai-2".into());
        let label = upsert_account(
            "openai",
            &mut accounts,
            &mut active,
            Account {
                label: "new".into(),
            },
            &HashMap::new(),
            |a| a.label.as_str(),
            |a, label| a.label = label,
        );
        assert_eq!(label, "openai-1");
        assert_eq!(active.as_deref(), Some("openai-2"));
    }

    #[test]
    fn relabel_accounts_preserves_labels_and_active_label() {
        let mut accounts = vec![
            Account {
                label: "default".to_string(),
            },
            Account {
                label: "other".to_string(),
            },
        ];
        let mut active = Some("other".to_string());

        let outcome = relabel_accounts(
            "openai",
            &mut accounts,
            &mut active,
            Some("default".to_string()),
            |account| account.label.as_str(),
            |account, label| account.label = label,
        );

        assert!(!outcome.changed);
        assert_eq!(accounts[0].label, "default");
        assert_eq!(accounts[1].label, "other");
        assert_eq!(active.as_deref(), Some("other"));
        assert_eq!(outcome.canonical_override_label.as_deref(), None);
    }

    #[test]
    fn upsert_account_assigns_next_label_and_sets_initial_active() {
        let mut accounts = Vec::<Account>::new();
        let mut active = None;

        let label = upsert_account(
            "claude",
            &mut accounts,
            &mut active,
            Account {
                label: "ignored".to_string(),
            },
            &HashMap::new(),
            |account| account.label.as_str(),
            |account, label| account.label = label,
        );

        assert_eq!(label, "claude-1");
        assert_eq!(accounts[0].label, "claude-1");
        assert_eq!(active.as_deref(), Some("claude-1"));
    }

    #[test]
    fn account_labels_use_numbers() {
        assert_eq!(canonical_account_label("claude", 1), "claude-1");
        assert_eq!(canonical_account_label("claude", 2), "claude-2");
        assert_eq!(canonical_account_label("openai", 32), "openai-32");
        assert_eq!(canonical_account_label("openai", 33), "openai-33");
    }
}
