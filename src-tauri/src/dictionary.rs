//! Local vocabulary. Project terms are hints; replacements are explicit, bounded,
//! whole-token and single-pass (a replacement can never trigger another rule).
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::Mutex,
};
use tauri::{AppHandle, Manager};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub heard: String,
    pub written: String,
    pub project_id: Option<String>,
    pub origin: String,
    pub enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Dictionary {
    pub revision: u64,
    pub entries: Vec<Entry>,
    pub suggestions: Vec<Entry>,
    pub projects: Vec<Project>,
    pub active_project: Option<String>,
    pub learning: String,
}
impl Default for Dictionary {
    fn default() -> Self {
        Self {
            revision: 0,
            entries: vec![],
            suggestions: vec![],
            projects: vec![],
            active_project: None,
            learning: "suggest".into(),
        }
    }
}
pub struct DictionaryStore {
    path: PathBuf,
    data: Mutex<Dictionary>,
}
impl DictionaryStore {
    pub fn load(path: PathBuf) -> Self {
        let data = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            path,
            data: Mutex::new(data),
        }
    }
    pub fn snapshot(&self) -> Dictionary {
        self.data.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    pub fn save(&self, mut next: Dictionary) -> Result<Dictionary, String> {
        validate(&mut next)?;
        let mut data = self.data.lock().unwrap_or_else(|p| p.into_inner());
        if data.revision != next.revision {
            return Err("Dictionary changed. Reopen it to load the latest version.".into());
        }
        next.revision += 1;
        crate::storage::write_atomic(
            &self.path,
            &serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?,
        )?;
        *data = next.clone();
        Ok(next)
    }
    pub fn learn(
        &self,
        before: &str,
        after: &str,
        project_id: Option<String>,
    ) -> Result<(), String> {
        let mut next = self.snapshot();
        if next.learning == "off" {
            return Ok(());
        }
        let Some((heard, written)) = correction(before, after) else {
            return Ok(());
        };
        if project_id
            .as_ref()
            .is_some_and(|id| !next.projects.iter().any(|p| &p.id == id))
        {
            return Ok(());
        }
        if next
            .entries
            .iter()
            .chain(next.suggestions.iter())
            .any(|e| e.project_id == project_id && e.heard.to_lowercase() == heard.to_lowercase())
        {
            return Ok(());
        }
        let entry = Entry {
            id: format!(
                "learned-{}",
                crate::error::temp_wav_path()
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
            ),
            heard,
            written,
            project_id,
            origin: "learned".into(),
            enabled: true,
        };
        if next.learning == "auto" {
            next.entries.push(entry);
        } else {
            next.suggestions.push(entry);
        }
        self.save(next).map(|_| ())
    }
}
fn validate(d: &mut Dictionary) -> Result<(), String> {
    if d.entries.len() > 2000 || d.suggestions.len() > 500 || d.projects.len() > 50 {
        return Err(
            "Dictionary limit reached (2,000 entries, 500 suggestions, 50 projects).".into(),
        );
    }
    if !["off", "suggest", "auto"].contains(&d.learning.as_str()) {
        return Err("Unknown learning mode".into());
    }
    let mut ids = HashSet::new();
    for p in &mut d.projects {
        p.name = p.name.trim().to_string();
        if p.id.is_empty()
            || p.id.len() > 100
            || !ids.insert(p.id.clone())
            || p.name.is_empty()
            || p.name.len() > 200
            || p.path.len() > 4096
        {
            return Err("Invalid project".into());
        }
    }
    if d.active_project
        .as_ref()
        .is_some_and(|id| !ids.contains(id))
    {
        return Err("Active project no longer exists".into());
    }
    let projects = ids;
    let mut ids = HashSet::new();
    let mut keys = HashSet::new();
    for e in d.entries.iter_mut().chain(d.suggestions.iter_mut()) {
        e.heard = e.heard.trim().to_string();
        e.written = e.written.trim().to_string();
        if e.id.is_empty()
            || e.id.len() > 160
            || !ids.insert(e.id.clone())
            || e.written.is_empty()
            || e.written.chars().count() > 120
            || e.heard.chars().count() > 120
            || e.heard
                .chars()
                .chain(e.written.chars())
                .any(char::is_control)
            || e.project_id
                .as_ref()
                .is_some_and(|id| !projects.contains(id))
            || !["manual", "learned", "project"].contains(&e.origin.as_str())
        {
            return Err("Invalid dictionary entry".into());
        }
        let key = (
            e.project_id.clone(),
            if e.heard.is_empty() {
                format!("term:{}", e.written.to_lowercase())
            } else {
                e.heard.to_lowercase()
            },
        );
        if !keys.insert(key) {
            return Err("This word already exists in this dictionary".into());
        }
    }
    Ok(())
}
impl Dictionary {
    pub fn active_entries(&self) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| {
                e.enabled && (e.project_id.is_none() || e.project_id == self.active_project)
            })
            .collect()
    }
    pub fn hints(&self) -> Vec<String> {
        let mut entries = self.active_entries();
        entries.sort_by_key(|e| e.project_id.is_none());
        let mut seen = HashSet::new();
        let mut length = 0;
        entries
            .into_iter()
            .filter_map(|e| {
                length += e.written.len();
                (length <= 1400 && seen.insert(e.written.to_lowercase())).then(|| e.written.clone())
            })
            .take(80)
            .collect()
    }
    pub fn apply(&self, input: &str) -> String {
        let mut rules = BTreeMap::new();
        // Project rules override personal rules with the same spoken form.
        let mut entries = self.active_entries();
        entries.sort_by_key(|e| e.project_id.is_some());
        for e in entries {
            if !e.heard.is_empty() {
                rules.insert(e.heard.to_lowercase(), e.written.clone());
            }
        }
        // Preserve the original technical-term defaults, but let explicit user
        // rules override them, including corrections to a default's written form.
        for (aliases, written) in [
            ("джит|гит", "git"),
            ("джитхаб|гитхаб", "GitHub"),
            ("питон|пайтон", "Python"),
            ("докер", "Docker"),
            ("юсефект|юзэффект|юзефект|усефект", "useEffect"),
            ("тайпскрипт", "TypeScript"),
            ("джаваскрипт|жаваскрипт", "JavaScript"),
            ("реакт", "React"),
            ("коммит", "commit"),
            ("ребейс", "rebase"),
            ("мердж|мерж", "merge"),
            ("бранч", "branch"),
            ("пуш", "push"),
            ("фронтенд", "frontend"),
            ("бэкенд", "backend"),
            ("хэллоу ворлд|хеллоу ворлд|хелло ворлд", "Hello World"),
        ] {
            let replacement = rules
                .get(&written.to_lowercase())
                .cloned()
                .unwrap_or_else(|| written.into());
            for alias in aliases.split('|') {
                rules
                    .entry(alias.into())
                    .or_insert_with(|| replacement.clone());
            }
        }
        if rules.is_empty() {
            return input.into();
        }
        let mut phrases: Vec<_> = rules.keys().collect();
        phrases.sort_by_key(|p| std::cmp::Reverse(p.len()));
        let pattern = format!(
            "(?i){}",
            phrases
                .iter()
                .map(|p| regex::escape(p))
                .collect::<Vec<_>>()
                .join("|")
        );
        let Ok(re) = regex::Regex::new(&pattern) else {
            return input.into();
        };
        let mut output = String::new();
        let mut cursor = 0;
        for m in re.find_iter(input) {
            let word = |c: char| c.is_alphanumeric() || c == '_';
            if input[..m.start()].chars().next_back().is_some_and(word)
                || input[m.end()..].chars().next().is_some_and(word)
            {
                continue;
            }
            output.push_str(&input[cursor..m.start()]);
            output.push_str(&rules[&m.as_str().to_lowercase()]);
            cursor = m.end();
        }
        output.push_str(&input[cursor..]);
        output
    }
}
// Learn only one short word/phrase correction, never a rewritten paragraph.
fn correction(before: &str, after: &str) -> Option<(String, String)> {
    let a: Vec<_> = before.split_whitespace().collect();
    let b: Vec<_> = after.split_whitespace().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let old = &a[prefix..a.len() - suffix];
    let new = &b[prefix..b.len() - suffix];
    if old.is_empty() || new.is_empty() || old.len() > 3 || new.len() > 3 {
        return None;
    }
    let trim = |s: String| {
        s.trim_matches(|c: char| ",.!?:;\"«»()".contains(c))
            .to_string()
    };
    let heard = trim(old.join(" "));
    let written = trim(new.join(" "));
    if heard == written
        || heard.chars().count() < 2
        || written.is_empty()
        || heard.len() > 120
        || written.len() > 120
        || !heard.chars().any(char::is_alphabetic)
        || !written.chars().any(char::is_alphabetic)
    {
        return None;
    }
    Some((heard, written))
}
#[derive(Serialize)]
pub struct ScanResult {
    pub terms: Vec<String>,
    pub files: usize,
    pub truncated: bool,
}
pub fn scan(path: &str) -> Result<ScanResult, String> {
    let root = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    if !root.is_dir() {
        return Err("Choose a project folder".into());
    }
    let declarations = regex::Regex::new(r"\b(?:class|interface|type|enum|struct|trait|fn|function|def|const)\s+([A-Za-z_][A-Za-z_0-9]{2,60})").unwrap();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut files = 0;
    let mut bytes = 0;
    let mut truncated = false;
    let walker = ignore::WalkBuilder::new(&root)
        .hidden(true)
        .follow_links(false)
        .max_depth(Some(16))
        .require_git(false)
        .filter_entry(|e| {
            ![
                "node_modules",
                "target",
                "dist",
                "build",
                "vendor",
                "venv",
                "__pycache__",
                "coverage",
            ]
            .contains(&e.file_name().to_string_lossy().as_ref())
        })
        .build();
    for item in walker {
        let Ok(item) = item else {
            continue;
        };
        if !item.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = item.path();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ![
            "ts", "tsx", "js", "jsx", "rs", "py", "go", "java", "cs", "swift", "vue", "svelte",
        ]
        .contains(&ext)
            && path.file_name().and_then(|s| s.to_str()) != Some("package.json")
        {
            continue;
        }
        let lower = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if ["secret", "credential", ".min.", "lock", "generated"]
            .iter()
            .any(|s| lower.contains(s))
        {
            continue;
        }
        let Ok(meta) = path.metadata() else {
            continue;
        };
        if meta.len() > 256 * 1024 {
            continue;
        }
        if files >= 2000 || bytes + meta.len() > 16 * 1024 * 1024 {
            truncated = true;
            break;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        files += 1;
        bytes += meta.len();
        let mut add = |name: &str| {
            if name.len() >= 3
                && name.len() <= 64
                && !["index", "main", "mod", "self", "config", "default"].contains(&name)
            {
                *counts.entry(name.into()).or_default() += 1;
            }
        };
        if ext != "json" {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                add(stem);
            }
            for c in declarations.captures_iter(&content) {
                add(&c[1]);
            }
        } else if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
            for key in ["dependencies", "devDependencies"] {
                if let Some(deps) = value[key].as_object() {
                    for name in deps.keys() {
                        add(name);
                    }
                }
            }
        }
    }
    let mut terms: Vec<_> = counts.into_iter().collect();
    terms.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    truncated |= terms.len() > 300;
    terms.truncate(300);
    Ok(ScanResult {
        terms: terms.into_iter().map(|(s, _)| s).collect(),
        files,
        truncated,
    })
}
#[tauri::command]
pub fn get_dictionary(app: AppHandle) -> Dictionary {
    app.state::<DictionaryStore>().snapshot()
}
#[tauri::command]
pub fn save_dictionary(app: AppHandle, dictionary: Dictionary) -> Result<Dictionary, String> {
    app.state::<DictionaryStore>().save(dictionary)
}
#[tauri::command]
pub async fn scan_project(path: String) -> Result<ScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || scan(&path))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(heard: &str, written: &str, project: Option<&str>) -> Entry {
        Entry {
            id: heard.into(),
            heard: heard.into(),
            written: written.into(),
            project_id: project.map(str::to_string),
            origin: "manual".into(),
            enabled: true,
        }
    }
    #[test]
    fn replacements_are_unicode_bounded_literal_and_non_cascading() {
        let d = Dictionary {
            entries: vec![
                entry("реакт", "React", None),
                entry("React", "WRONG", None),
                entry("си плюс", "C++", None),
            ],
            ..Default::default()
        };
        assert_eq!(
            d.apply("РЕАКТ, реактор и си плюс!"),
            "React, реактор и C++!"
        );
    }
    #[test]
    fn projects_override_and_other_projects_never_leak() {
        let d = Dictionary {
            active_project: Some("a".into()),
            entries: vec![
                entry("foo", "personal", None),
                entry("foo", "project", Some("a")),
                entry("bar", "secret", Some("b")),
            ],
            ..Default::default()
        };
        assert_eq!(d.apply("foo bar"), "project bar");
        assert!(!d.hints().contains(&"secret".into()));
    }
    #[test]
    fn learning_ignores_deletions_and_paragraph_rewrites() {
        assert_eq!(
            correction("Нужен супабейс здесь.", "Нужен Supabase здесь."),
            Some(("супабейс".into(), "Supabase".into()))
        );
        assert_eq!(correction("hello world", "hello"), None);
        assert_eq!(correction("a b c d e", "one two three four five"), None);
        assert_eq!(correction("Привет мир", "Привет мир!"), None);
    }
    #[test]
    fn learned_rules_override_legacy_terms() {
        let d = Dictionary {
            entries: vec![entry("Docker", "докер", None)],
            ..Default::default()
        };
        assert_eq!(d.apply("докер и Docker"), "докер и докер");
    }
    #[test]
    fn store_persists_and_rejects_stale_writes() {
        let path = crate::error::temp_wav_path().with_extension("json");
        let store = DictionaryStore::load(path.clone());
        let stale = store.snapshot();
        let mut next = stale.clone();
        next.entries.push(entry("супабейс", "Supabase", None));
        store.save(next).unwrap();
        assert!(store.save(stale).is_err());
        assert_eq!(
            DictionaryStore::load(path.clone()).snapshot().entries[0].written,
            "Supabase"
        );
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn learning_modes_have_distinct_review_behavior() {
        let path = crate::error::temp_wav_path().with_extension("json");
        let store = DictionaryStore::load(path.clone());
        store
            .learn("нужен супабейс", "нужен Supabase", None)
            .unwrap();
        assert_eq!(store.snapshot().suggestions.len(), 1);
        assert!(store.snapshot().entries.is_empty());
        let mut next = store.snapshot();
        next.learning = "auto".into();
        store.save(next).unwrap();
        store
            .learn("используй версель", "используй Vercel", None)
            .unwrap();
        assert_eq!(store.snapshot().entries.len(), 1);
        let mut next = store.snapshot();
        next.learning = "off".into();
        store.save(next).unwrap();
        store
            .learn("нужен кубер", "нужен Kubernetes", None)
            .unwrap();
        assert_eq!(store.snapshot().entries.len(), 1);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn project_scan_excludes_hidden_ignored_dependencies_and_secrets() {
        let root = crate::error::temp_wav_path().with_extension("project");
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::write(root.join(".gitignore"), "ignored.ts\n").unwrap();
        std::fs::write(
            root.join("useAuth.ts"),
            "export function useAuth() {}\nconst refreshToken = 'not-a-term';",
        )
        .unwrap();
        std::fs::write(root.join("ignored.ts"), "const MustBeIgnored = 1;").unwrap();
        std::fs::write(root.join(".private.ts"), "const HiddenValue = 1;").unwrap();
        std::fs::write(root.join("credentials.ts"), "const SecretValue = 1;").unwrap();
        std::fs::write(
            root.join("node_modules/dependency.ts"),
            "const DependencyValue = 1;",
        )
        .unwrap();
        let result = scan(root.to_str().unwrap()).unwrap();
        assert_eq!(result.files, 1);
        assert!(result.terms.contains(&"useAuth".into()));
        assert!(result.terms.contains(&"refreshToken".into()));
        assert!(!result.terms.contains(&"not-a-term".into()));
        std::fs::remove_dir_all(root).unwrap();
    }
}
