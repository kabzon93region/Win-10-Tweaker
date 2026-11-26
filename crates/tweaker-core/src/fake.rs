//! In-memory adapters.
//!
//! Every engine test runs on these: no registry, no files, no services, no
//! administrator rights. A fake can be told to fail or to ignore writes, which is
//! how the error and verification paths are covered.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::action::{RegistryKey, RegistryValue, ServiceStartMode};
use crate::adapters::{
    AdapterError, AppxPackage, CleanStats, FirewallRule, IAppx, IFileSystem, IFirewall,
    IProcessRunner, IRegistry, IScheduler, IServiceControl, ISystemInfo, OsVersion, ProcessOutput,
    RegistryTree, ServiceState, ServiceStatus,
};

fn failure(message: &str) -> AdapterError {
    AdapterError::new(message)
}

/// In-memory registry.
#[derive(Default)]
pub struct FakeRegistry {
    values: RefCell<HashMap<(crate::action::RegistryRoot, String), HashMap<String, RegistryValue>>>,
    /// When set, every write and delete fails with this text.
    pub fail: RefCell<Option<String>>,
    /// When set, writes report success but do not store anything (verification test).
    pub ignore_writes: RefCell<bool>,
}

impl FakeRegistry {
    /// Preloads a value without touching the failure switches.
    pub fn set(&self, key: &RegistryKey, name: &str, value: RegistryValue) {
        self.values
            .borrow_mut()
            .entry((key.root, key.path.clone()))
            .or_default()
            .insert(name.to_owned(), value);
    }

    /// Reads a value directly (test assertions).
    pub fn get(&self, key: &RegistryKey, name: &str) -> Option<RegistryValue> {
        self.values
            .borrow()
            .get(&(key.root, key.path.clone()))
            .and_then(|values| values.get(name).cloned())
    }

    /// Creates an empty key (no values), as some shell keys are.
    pub fn ensure_key(&self, key: &RegistryKey) {
        self.values
            .borrow_mut()
            .entry((key.root, key.path.clone()))
            .or_default();
    }
}

impl IRegistry for FakeRegistry {
    fn read(&self, key: &RegistryKey, name: &str) -> Result<Option<RegistryValue>, AdapterError> {
        Ok(self.get(key, name))
    }

    fn write(
        &self,
        key: &RegistryKey,
        name: &str,
        value: &RegistryValue,
    ) -> Result<(), AdapterError> {
        if let Some(message) = self.fail.borrow().as_ref() {
            return Err(failure(message));
        }
        if *self.ignore_writes.borrow() {
            return Ok(());
        }
        self.values
            .borrow_mut()
            .entry((key.root, key.path.clone()))
            .or_default()
            .insert(name.to_owned(), value.clone());
        Ok(())
    }

    fn delete(&self, key: &RegistryKey, name: &str) -> Result<(), AdapterError> {
        if let Some(message) = self.fail.borrow().as_ref() {
            return Err(failure(message));
        }
        if *self.ignore_writes.borrow() {
            return Ok(());
        }
        if let Some(values) = self
            .values
            .borrow_mut()
            .get_mut(&(key.root, key.path.clone()))
        {
            values.remove(name);
        }
        Ok(())
    }

    fn read_tree(&self, key: &RegistryKey) -> Result<Option<RegistryTree>, AdapterError> {
        let values = self.values.borrow();
        if !values.contains_key(&(key.root, key.path.clone()))
            && !values
                .keys()
                .any(|(root, path)| *root == key.root && path.starts_with(&child_prefix(&key.path)))
        {
            return Ok(None);
        }
        Ok(Some(tree_for(key.root, &key.path, &values)))
    }

    fn delete_tree(&self, key: &RegistryKey) -> Result<(), AdapterError> {
        if let Some(message) = self.fail.borrow().as_ref() {
            return Err(failure(message));
        }
        if *self.ignore_writes.borrow() {
            return Ok(());
        }
        if key.path.is_empty() {
            return Err(failure("refusing to delete a registry root"));
        }
        let prefix = child_prefix(&key.path);
        self.values.borrow_mut().retain(|(root, path), _| {
            *root != key.root || (path != &key.path && !path.starts_with(&prefix))
        });
        Ok(())
    }
}

fn child_prefix(path: &str) -> String {
    if path.is_empty() {
        String::new()
    } else {
        format!("{path}\\")
    }
}

fn tree_for(
    root: crate::action::RegistryRoot,
    base: &str,
    values: &HashMap<(crate::action::RegistryRoot, String), HashMap<String, RegistryValue>>,
) -> RegistryTree {
    let mut own: Vec<(String, RegistryValue)> = values
        .get(&(root, base.to_owned()))
        .map(|map| {
            map.iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default();
    own.sort_by(|left, right| left.0.cmp(&right.0));

    let prefix = child_prefix(base);
    let mut names: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for (root_of_key, path) in values.keys() {
        if *root_of_key != root || path == base || !path.starts_with(&prefix) {
            continue;
        }
        let rest = &path[prefix.len()..];
        let Some(first) = rest.split('\\').next() else {
            continue;
        };
        let child_base = if base.is_empty() {
            first.to_owned()
        } else {
            format!("{base}\\{first}")
        };
        names.insert(first.to_owned(), child_base);
    }
    let subkeys = names
        .into_iter()
        .map(|(name, child_base)| (name, tree_for(root, &child_base, values)))
        .collect();
    RegistryTree {
        values: own,
        subkeys,
    }
}

/// In-memory text file system.
#[derive(Default)]
pub struct FakeFileSystem {
    files: RefCell<HashMap<PathBuf, String>>,
    bytes: RefCell<HashMap<PathBuf, Vec<u8>>>,
    /// When set, every write and delete fails with this text.
    pub fail: RefCell<Option<String>>,
    /// Files that behave as locked and are skipped by `clean`.
    pub locked: RefCell<HashSet<PathBuf>>,
}

impl FakeFileSystem {
    /// Preloads a file.
    pub fn put(&self, path: impl Into<PathBuf>, content: impl Into<String>) {
        self.files.borrow_mut().insert(path.into(), content.into());
    }

    /// Preloads a binary file.
    pub fn put_bytes(&self, path: impl Into<PathBuf>, content: impl Into<Vec<u8>>) {
        self.bytes.borrow_mut().insert(path.into(), content.into());
    }

    /// Reads a file directly (test assertions).
    pub fn get(&self, path: &Path) -> Option<String> {
        self.files.borrow().get(path).cloned()
    }

    /// Reads binary contents directly (test assertions).
    pub fn get_bytes(&self, path: &Path) -> Option<Vec<u8>> {
        self.bytes.borrow().get(path).cloned()
    }
}

impl IFileSystem for FakeFileSystem {
    fn exists(&self, path: &Path) -> bool {
        self.files.borrow().contains_key(path) || self.bytes.borrow().contains_key(path)
    }

    fn read_text(&self, path: &Path) -> Result<Option<String>, AdapterError> {
        Ok(self.get(path))
    }

    fn write_text(&self, path: &Path, content: &str) -> Result<(), AdapterError> {
        if let Some(message) = self.fail.borrow().as_ref() {
            return Err(failure(message));
        }
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), content.to_owned());
        Ok(())
    }

    fn read_bytes(&self, path: &Path) -> Result<Option<Vec<u8>>, AdapterError> {
        Ok(self.get_bytes(path))
    }

    fn write_bytes(&self, path: &Path, bytes: &[u8]) -> Result<(), AdapterError> {
        if let Some(message) = self.fail.borrow().as_ref() {
            return Err(failure(message));
        }
        self.bytes
            .borrow_mut()
            .insert(path.to_path_buf(), bytes.to_vec());
        Ok(())
    }

    fn delete(&self, path: &Path) -> Result<(), AdapterError> {
        if let Some(message) = self.fail.borrow().as_ref() {
            return Err(failure(message));
        }
        self.files.borrow_mut().remove(path);
        self.bytes.borrow_mut().remove(path);
        Ok(())
    }

    fn measure(&self, paths: &[PathBuf]) -> Result<u64, AdapterError> {
        let files = self.files.borrow();
        let bytes = self.bytes.borrow();
        let mut total = 0u64;
        for path in paths {
            for (file, content) in files.iter() {
                if file.starts_with(path) {
                    total += content.len() as u64;
                }
            }
            for (file, content) in bytes.iter() {
                if file.starts_with(path) {
                    total += content.len() as u64;
                }
            }
        }
        Ok(total)
    }

    fn clean(&self, paths: &[PathBuf]) -> Result<CleanStats, AdapterError> {
        if let Some(message) = self.fail.borrow().as_ref() {
            return Err(failure(message));
        }
        let mut stats = CleanStats::default();
        let locked = self.locked.borrow().clone();
        for path in paths {
            let mut files = self.files.borrow_mut();
            let doomed: Vec<PathBuf> = files
                .keys()
                .filter(|file| file.starts_with(path) && !locked.contains(*file))
                .cloned()
                .collect();
            for file in doomed {
                if let Some(content) = files.remove(&file) {
                    stats.freed_bytes += content.len() as u64;
                }
            }
            stats.skipped_files += locked.iter().filter(|file| file.starts_with(path)).count();
        }
        for path in paths {
            let mut bytes = self.bytes.borrow_mut();
            let doomed: Vec<PathBuf> = bytes
                .keys()
                .filter(|file| file.starts_with(path) && !locked.contains(*file))
                .cloned()
                .collect();
            for file in doomed {
                if let Some(content) = bytes.remove(&file) {
                    stats.freed_bytes += content.len() as u64;
                }
            }
        }
        Ok(stats)
    }
}

/// In-memory service manager.
#[derive(Default)]
pub struct FakeServiceControl {
    services: RefCell<HashMap<String, ServiceState>>,
    /// When set, every start/stop/configure call fails with this text.
    pub fail: RefCell<Option<String>>,
}

impl FakeServiceControl {
    /// Preloads a service.
    pub fn add(&self, name: &str, state: ServiceState) {
        self.services.borrow_mut().insert(name.to_owned(), state);
    }

    /// Removes a service, as if it was uninstalled.
    pub fn remove(&self, name: &str) {
        self.services.borrow_mut().remove(name);
    }
}

impl IServiceControl for FakeServiceControl {
    fn query(&self, service: &str) -> Result<Option<ServiceState>, AdapterError> {
        Ok(self.services.borrow().get(service).copied())
    }

    fn start(&self, service: &str) -> Result<(), AdapterError> {
        if let Some(message) = self.fail.borrow().as_ref() {
            return Err(failure(message));
        }
        let mut services = self.services.borrow_mut();
        let state = services
            .get_mut(service)
            .ok_or_else(|| failure(&format!("service {service} is not installed")))?;
        if state.status == ServiceStatus::Running {
            return Err(failure(&format!("service {service} is already running")));
        }
        state.status = ServiceStatus::Running;
        Ok(())
    }

    fn stop(&self, service: &str) -> Result<(), AdapterError> {
        if let Some(message) = self.fail.borrow().as_ref() {
            return Err(failure(message));
        }
        let mut services = self.services.borrow_mut();
        let state = services
            .get_mut(service)
            .ok_or_else(|| failure(&format!("service {service} is not installed")))?;
        if state.status == ServiceStatus::Stopped {
            return Err(failure(&format!("service {service} is already stopped")));
        }
        state.status = ServiceStatus::Stopped;
        Ok(())
    }

    fn set_start_mode(&self, service: &str, mode: ServiceStartMode) -> Result<(), AdapterError> {
        if let Some(message) = self.fail.borrow().as_ref() {
            return Err(failure(message));
        }
        let mut services = self.services.borrow_mut();
        let state = services
            .get_mut(service)
            .ok_or_else(|| failure(&format!("service {service} is not installed")))?;
        state.start_mode = mode;
        Ok(())
    }
}

/// In-memory program runner.
pub struct FakeProcessRunner {
    /// Output returned by [`IProcessRunner::run`].
    pub output: RefCell<ProcessOutput>,
    /// Everything that was run, in order.
    pub runs: RefCell<Vec<(String, Vec<String>)>>,
}

impl Default for FakeProcessRunner {
    fn default() -> Self {
        Self {
            output: RefCell::new(ProcessOutput {
                code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            }),
            runs: RefCell::new(Vec::new()),
        }
    }
}

impl IProcessRunner for FakeProcessRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<ProcessOutput, AdapterError> {
        self.runs
            .borrow_mut()
            .push((program.to_owned(), args.to_vec()));
        Ok(self.output.borrow().clone())
    }
}

/// In-memory firewall.
#[derive(Default)]
pub struct FakeFirewall {
    rules: RefCell<HashSet<String>>,
}

impl IFirewall for FakeFirewall {
    fn ensure_rule(&self, rule: &FirewallRule) -> Result<(), AdapterError> {
        self.rules.borrow_mut().insert(rule.name.clone());
        Ok(())
    }

    fn remove_rule(&self, name: &str) -> Result<(), AdapterError> {
        self.rules.borrow_mut().remove(name);
        Ok(())
    }

    fn has_rule(&self, name: &str) -> Result<bool, AdapterError> {
        Ok(self.rules.borrow().contains(name))
    }
}

/// In-memory Task Scheduler.
#[derive(Default)]
pub struct FakeScheduler {
    tasks: RefCell<HashMap<String, bool>>,
}

impl FakeScheduler {
    /// Preloads a task with its enabled state.
    pub fn add(&self, path: &str, enabled: bool) {
        self.tasks.borrow_mut().insert(path.to_owned(), enabled);
    }
}

impl IScheduler for FakeScheduler {
    fn list(&self, folder: &str) -> Result<Vec<String>, AdapterError> {
        let trimmed = folder.trim_end_matches('\\');
        let prefix = if trimmed.is_empty() {
            "\\".to_owned()
        } else {
            format!("{trimmed}\\")
        };
        let mut paths: Vec<String> = self
            .tasks
            .borrow()
            .keys()
            .filter(|path| path.starts_with(&prefix))
            .cloned()
            .collect();
        paths.sort();
        Ok(paths)
    }

    fn set_enabled(&self, task_path: &str, enabled: bool) -> Result<(), AdapterError> {
        let mut tasks = self.tasks.borrow_mut();
        let state = tasks
            .get_mut(task_path)
            .ok_or_else(|| failure(&format!("task {task_path} was not found")))?;
        *state = enabled;
        Ok(())
    }

    fn is_enabled(&self, task_path: &str) -> Result<Option<bool>, AdapterError> {
        Ok(self.tasks.borrow().get(task_path).copied())
    }
}

/// In-memory Store packages.
#[derive(Default)]
pub struct FakeAppx {
    packages: RefCell<HashMap<String, AppxPackage>>,
    /// When set, every removal fails with this text.
    pub fail_remove: RefCell<Option<String>>,
    /// Manifests passed to `register`, in order.
    pub registered: RefCell<Vec<PathBuf>>,
}

impl FakeAppx {
    /// Preloads an installed package family with a conventional manifest path.
    pub fn add(&self, family: &str) {
        let manifest = PathBuf::from(format!(r"C:\Apps\{family}\AppxManifest.xml"));
        self.packages.borrow_mut().insert(
            family.to_owned(),
            AppxPackage {
                family: family.to_owned(),
                full_name: format!("{family}_1.0.0.0_x64__test"),
                manifest,
            },
        );
    }

    /// Preloads a package with an explicit manifest path.
    pub fn add_package(&self, package: AppxPackage) {
        self.packages
            .borrow_mut()
            .insert(package.family.clone(), package);
    }

    /// Whether `register` was called with this manifest.
    pub fn was_registered(&self, manifest: &Path) -> bool {
        self.registered.borrow().iter().any(|path| path == manifest)
    }

    fn find(&self, package_family: &str) -> Option<(String, AppxPackage)> {
        let token = package_family.to_lowercase();
        self.packages
            .borrow()
            .iter()
            .find(|(family, _)| family.to_lowercase().contains(&token))
            .map(|(family, package)| (family.clone(), package.clone()))
    }
}

impl IAppx for FakeAppx {
    fn installed(&self, package_family: &str) -> Result<bool, AdapterError> {
        Ok(self.find(package_family).is_some())
    }

    fn package(&self, package_family: &str) -> Result<Option<AppxPackage>, AdapterError> {
        Ok(self.find(package_family).map(|(_, package)| package))
    }

    fn remove(&self, package_family: &str) -> Result<(), AdapterError> {
        if let Some(reason) = self.fail_remove.borrow().as_deref() {
            return Err(failure(reason));
        }
        match self.find(package_family) {
            Some((family, _)) => {
                self.packages.borrow_mut().remove(&family);
                Ok(())
            }
            None => Err(failure(&format!(
                "package {package_family} is not installed"
            ))),
        }
    }

    fn register(&self, manifest: &Path) -> Result<(), AdapterError> {
        self.registered.borrow_mut().push(manifest.to_path_buf());
        let family = manifest
            .parent()
            .and_then(|parent| parent.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "restored.package".to_owned());
        self.packages.borrow_mut().insert(
            family.clone(),
            AppxPackage {
                family: family.clone(),
                full_name: format!("{family}_1.0.0.0_x64__restored"),
                manifest: manifest.to_path_buf(),
            },
        );
        Ok(())
    }
}

/// Fixed OS version.
#[derive(Debug, Clone, Copy)]
pub struct FakeSystemInfo {
    /// Version reported to the engine.
    pub version: OsVersion,
}

impl Default for FakeSystemInfo {
    fn default() -> Self {
        Self {
            version: OsVersion {
                major: 10,
                minor: 0,
                build: 19045,
            },
        }
    }
}

impl ISystemInfo for FakeSystemInfo {
    fn os_version(&self) -> Result<OsVersion, AdapterError> {
        Ok(self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::RegistryRoot;

    #[test]
    fn registry_fake_round_trips_values() {
        let registry = FakeRegistry::default();
        let key = RegistryKey::new(RegistryRoot::Hkcu, "Software\\Test");
        assert_eq!(registry.read(&key, "X").expect("read"), None);
        registry
            .write(&key, "X", &RegistryValue::Dword(7))
            .expect("write");
        assert_eq!(
            registry.read(&key, "X").expect("read"),
            Some(RegistryValue::Dword(7))
        );
        registry.delete(&key, "X").expect("delete");
        assert_eq!(registry.read(&key, "X").expect("read"), None);
    }

    #[test]
    fn failure_switches_are_visible() {
        let registry = FakeRegistry::default();
        *registry.fail.borrow_mut() = Some("access denied".to_owned());
        let key = RegistryKey::new(RegistryRoot::Hklm, "Software\\Test");
        let error = registry
            .write(&key, "X", &RegistryValue::Dword(1))
            .expect_err("must fail");
        assert_eq!(error.message, "access denied");
    }

    #[test]
    fn service_fake_changes_state() {
        let services = FakeServiceControl::default();
        services.add(
            "Demo",
            ServiceState {
                status: ServiceStatus::Stopped,
                start_mode: ServiceStartMode::Manual,
            },
        );
        services.start("Demo").expect("start");
        let state = services.query("Demo").expect("query").expect("state");
        assert_eq!(state.status, ServiceStatus::Running);
        services.stop("Demo").expect("stop");
        let state = services.query("Demo").expect("query").expect("state");
        assert_eq!(state.status, ServiceStatus::Stopped);
    }
}
