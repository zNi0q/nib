//! Sidebar file tree: a flat list of entries where expanding a folder
//! inserts its children right after it.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const HIDDEN: &[&str] = &[".git"];

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

pub struct Tree {
    pub root: PathBuf,
    pub items: Vec<Entry>,
    pub sel: usize,
    pub scroll: usize,
}

fn children(dir: &Path, depth: usize) -> Vec<Entry> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Entry> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if HIDDEN.contains(&name.as_str()) {
                return None;
            }
            let path = e.path();
            // Follows symlinks, so a link to a folder shows as a folder.
            let is_dir = path.is_dir();
            Some(Entry { path, name, depth, is_dir, expanded: false })
        })
        .collect();
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

impl Tree {
    pub fn new(root: PathBuf) -> Self {
        let items = children(&root, 0);
        Tree { root, items, sel: 0, scroll: 0 }
    }

    pub fn selected(&self) -> Option<&Entry> {
        self.items.get(self.sel)
    }

    pub fn expand(&mut self, i: usize) {
        let e = &self.items[i];
        if !e.is_dir || e.expanded {
            return;
        }
        let kids = children(&e.path, e.depth + 1);
        self.items[i].expanded = true;
        self.items.splice(i + 1..i + 1, kids);
    }

    pub fn collapse(&mut self, i: usize) {
        let depth = self.items[i].depth;
        let end = self.items[i + 1..].iter().position(|e| e.depth <= depth).map_or(self.items.len(), |n| i + 1 + n);
        self.items.drain(i + 1..end);
        self.items[i].expanded = false;
        if self.sel > i {
            self.sel = if self.sel < end { i } else { self.sel - (end - i - 1) };
        }
    }

    pub fn toggle(&mut self, i: usize) {
        if self.items[i].expanded { self.collapse(i) } else { self.expand(i) }
    }

    /// Select the parent folder of the selected entry.
    pub fn select_parent(&mut self) {
        let Some(depth) = self.selected().map(|e| e.depth) else { return };
        if let Some(p) = self.items[..self.sel].iter().rposition(|e| e.depth < depth) {
            self.sel = p;
        }
    }

    pub fn move_sel(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        self.sel = (self.sel as isize + delta).clamp(0, self.items.len() as isize - 1) as usize;
    }

    /// Re-read the disk, keeping expanded folders and the selection.
    pub fn refresh(&mut self) {
        let open: HashSet<PathBuf> = self.items.iter().filter(|e| e.expanded).map(|e| e.path.clone()).collect();
        let sel = self.selected().map(|e| e.path.clone());
        self.items = children(&self.root, 0);
        let mut i = 0;
        while i < self.items.len() {
            if open.contains(&self.items[i].path) {
                self.expand(i);
            }
            i += 1;
        }
        self.sel = sel.and_then(|p| self.items.iter().position(|e| e.path == p)).unwrap_or(0).min(self.items.len().saturating_sub(1));
    }

    /// Expand the folders leading to `path` and select it.
    pub fn reveal(&mut self, path: &Path) {
        let Ok(rel) = path.strip_prefix(&self.root) else { return };
        let mut cur = self.root.clone();
        for part in rel.components() {
            cur.push(part);
            let Some(i) = self.items.iter().position(|e| e.path == cur) else { return };
            self.sel = i;
            self.expand(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nib-tree-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        for p in ["src/ui", ".git/objects", "docs"] {
            fs::create_dir_all(d.join(p)).unwrap();
        }
        for f in ["b.txt", "A.md", "src/main.rs", "src/ui/view.rs", ".env"] {
            fs::write(d.join(f), "").unwrap();
        }
        d
    }

    fn names(t: &Tree) -> Vec<String> {
        t.items.iter().map(|e| format!("{}{}", "  ".repeat(e.depth), e.name)).collect()
    }

    #[test]
    fn folders_first_sorted_git_hidden_expand_collapse() {
        let d = fixture("sort");
        let mut t = Tree::new(d.clone());
        assert_eq!(names(&t), ["docs", "src", ".env", "A.md", "b.txt"]);
        t.expand(1);
        t.expand(2);
        assert_eq!(names(&t), ["docs", "src", "  ui", "    view.rs", "  main.rs", ".env", "A.md", "b.txt"]);
        t.sel = 4;
        t.select_parent();
        assert_eq!(t.sel, 1);
        t.sel = 6;
        t.collapse(1);
        assert_eq!(names(&t), ["docs", "src", ".env", "A.md", "b.txt"]);
        assert_eq!(t.items[t.sel].name, "A.md", "selection follows the same entry");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn reveal_and_refresh() {
        let d = fixture("reveal");
        let mut t = Tree::new(d.clone());
        t.reveal(&d.join("src/ui/view.rs"));
        assert_eq!(t.items[t.sel].name, "view.rs");
        fs::write(d.join("src/new.rs"), "").unwrap();
        t.refresh();
        assert!(names(&t).contains(&"  new.rs".to_string()));
        assert_eq!(t.items[t.sel].name, "view.rs");
        fs::remove_dir_all(d).unwrap();
    }
}
