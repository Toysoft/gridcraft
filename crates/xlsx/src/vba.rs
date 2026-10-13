//! The VBA project part (`xl/vbaProject.bin`, a compound file): module source and UserForm
//! designs, for running macros. The bytes themselves are kept on the workbook and saved as read.

use gridcraft_vba::forms::{FormDesign, read_form};
use gridcraft_vba::ovba::{ModuleKind, Project, read_project};

use crate::cfb::Compound;

/// A workbook's macros: the project's modules and its UserForms.
#[derive(Clone, Debug, Default)]
pub struct VbaContents {
    pub project: Project,
    pub forms: Vec<FormDesign>,
}

/// Reads `vbaProject.bin`.
pub fn read_vba(bin: &[u8]) -> Result<VbaContents, String> {
    let cf = Compound::open(bin)?;
    let mut stream = |path: &str| cf.stream(path).ok().flatten();
    let project = read_project(&mut stream).map_err(|e| e.to_string())?;
    let mut forms = Vec::new();
    for m in project.modules.iter().filter(|m| m.kind == ModuleKind::Form) {
        let storage = &m.stream;
        let get = |s: &str| cf.stream(&format!("{storage}/{s}")).ok().flatten().unwrap_or_default();
        let (vb, f, o) = (get("\u{3}VBFrame"), get("f"), get("o"));
        if f.is_empty() {
            continue;
        }
        forms.push(read_form(&m.name, &vb, &f, &o, project.code_page));
    }
    Ok(VbaContents { project, forms })
}

#[cfg(test)]
mod tests {
    #[test]
    fn junk_is_an_error_not_a_panic() {
        assert!(super::read_vba(b"not a compound file").is_err());
        assert!(super::read_vba(&[]).is_err());
    }
}

#[cfg(test)]
mod corpus {
    /// Prints what a real `vbaProject.bin` holds (`GRIDCRAFT_VBA_BIN=path`).
    #[test]
    #[ignore = "needs GRIDCRAFT_VBA_BIN"]
    fn dump_project() {
        let Some(p) = std::env::var_os("GRIDCRAFT_VBA_BIN") else { return };
        let v = super::read_vba(&std::fs::read(p).unwrap()).unwrap();
        for m in &v.project.modules {
            println!("module {} {:?} {} lines", m.name, m.kind, m.code.lines().count());
        }
        for f in &v.forms {
            println!("form {} {:?} {}x{} back={:?} font={:?}", f.name, f.caption, f.width, f.height, f.back, f.font);
            for c in &f.controls {
                println!(
                    "   {:?} {} @({:.0},{:.0}) {:.0}x{:.0} cap={:?} val={:?} fore={:?} back={:?} font={:?} rowsrc={:?}",
                    c.kind,
                    c.name,
                    c.left,
                    c.top,
                    c.width,
                    c.height,
                    c.caption,
                    c.value,
                    c.fore,
                    c.back,
                    c.font.as_ref().map(|f| (&f.name, f.size)),
                    c.row_source
                );
            }
        }
    }
}
