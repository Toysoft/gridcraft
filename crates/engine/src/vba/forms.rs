//! UserForms at run time: each loaded form's controls and their state (text, list items,
//! selection, colours, visibility), the members VBA reads and writes, and the events those writes
//! fire. The UI draws [`FormState`] and reports what the user does; macros change it through
//! [`get`] and [`set`].

use gridcraft_vba::forms::{ControlDesign, ControlKind, FormDesign};
use gridcraft_vba::{Event, HostArg, Value, VbaError};

/// A control's live state.
#[derive(Clone, Debug, PartialEq)]
pub struct ControlState {
    pub design: ControlDesign,
    /// TextBox / ComboBox text, Label / CommandButton caption source for `Value` on those.
    pub text: String,
    pub caption: String,
    /// CheckBox / OptionButton / ToggleButton.
    pub checked: bool,
    /// SpinButton / ScrollBar.
    pub position: i32,
    /// List rows (ListBox / ComboBox), each with its columns.
    pub list: Vec<Vec<String>>,
    /// Selected row (-1: none).
    pub list_index: i64,
    /// Multi-select rows.
    pub selected: Vec<bool>,
    pub enabled: bool,
    pub visible: bool,
    pub locked: bool,
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    pub fore: Option<u32>,
    pub back: Option<u32>,
    pub font_size: Option<f32>,
    pub bold: Option<bool>,
    pub tag: String,
    pub column_count: i32,
    pub column_widths: String,
    pub sel_start: usize,
    pub sel_length: usize,
}

impl ControlState {
    fn new(d: &ControlDesign) -> ControlState {
        ControlState {
            design: d.clone(),
            text: d.value.clone(),
            caption: d.caption.clone(),
            checked: d.value == "1" || d.value.eq_ignore_ascii_case("true"),
            position: d.position,
            list: Vec::new(),
            list_index: -1,
            selected: Vec::new(),
            enabled: d.enabled,
            visible: d.visible,
            locked: d.locked,
            left: d.left,
            top: d.top,
            width: d.width,
            height: d.height,
            fore: d.fore,
            back: d.back,
            font_size: None,
            bold: None,
            tag: String::new(),
            column_count: d.column_count.max(1),
            column_widths: String::new(),
            sel_start: 0,
            sel_length: 0,
        }
    }

    pub fn name(&self) -> &str {
        &self.design.name
    }
    pub fn kind(&self) -> ControlKind {
        self.design.kind
    }

    /// `Value` as VBA sees it.
    pub fn value(&self) -> Value {
        match self.kind() {
            ControlKind::CheckBox | ControlKind::OptionButton | ControlKind::ToggleButton => Value::Bool(self.checked),
            ControlKind::SpinButton | ControlKind::ScrollBar => Value::Int(i64::from(self.position)),
            ControlKind::ListBox => match self.list_row() {
                Some(row) => Value::from(row.first().cloned().unwrap_or_default()),
                None => Value::Null,
            },
            ControlKind::Label | ControlKind::CommandButton => Value::from(self.caption.clone()),
            _ => Value::from(self.text.clone()),
        }
    }

    fn list_row(&self) -> Option<&Vec<String>> {
        usize::try_from(self.list_index).ok().and_then(|i| self.list.get(i))
    }
}

/// A loaded UserForm.
#[derive(Clone, Debug, PartialEq)]
pub struct FormState {
    pub design: FormDesign,
    pub caption: String,
    pub visible: bool,
    /// Shown modally (a macro is waiting in `.Show`).
    pub modal: bool,
    pub controls: Vec<ControlState>,
    /// Index of the control with the focus.
    pub focus: Option<usize>,
    pub back: Option<u32>,
    pub width: f32,
    pub height: f32,
    /// Order the form was shown in (the UI stacks forms by it).
    pub shown_order: u64,
    /// Unloaded: kept only so its `UserForm_Terminate` handler can still use `Me`.
    pub unloaded: bool,
}

impl FormState {
    pub fn new(design: &FormDesign) -> FormState {
        let mut controls: Vec<ControlState> = design.controls.iter().map(ControlState::new).collect();
        controls.sort_by_key(|c| c.design.tab_index);
        FormState {
            design: design.clone(),
            caption: design.caption.clone(),
            visible: false,
            modal: false,
            focus: controls.iter().position(|c| c.design.tab_stop && c.visible && c.enabled && !matches!(c.kind(), ControlKind::Label)),
            controls,
            back: design.back,
            width: design.width,
            height: design.height,
            shown_order: 0,
            unloaded: false,
        }
    }

    pub fn name(&self) -> &str {
        &self.design.name
    }

    pub fn control(&self, name: &str) -> Option<usize> {
        self.controls.iter().position(|c| c.name().eq_ignore_ascii_case(name))
    }
}

fn ev(form: &str, proc: String, args: Vec<Value>) -> Event {
    Event { module: form.to_string(), proc, args }
}

fn arg(args: &[HostArg], i: usize, name: &str) -> Option<Value> {
    args.iter()
        .find(|a| a.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .or_else(|| args.get(i).filter(|a| a.name.is_none()))
        .map(|a| a.value.clone())
        .filter(|v| !matches!(v, Value::Missing))
}

fn not_supported(what: &str, m: &str) -> VbaError {
    VbaError::runtime(438, format!("Object doesn't support this property or method: {what}.{m}"))
}

fn index_arg(v: Option<Value>) -> Result<Option<i64>, VbaError> {
    v.map(|v| v.to_int()).transpose()
}

/// A colour from VBA (`RGB(…)` is 0x00BBGGRR).
fn rgb_of(v: &Value) -> Result<u32, VbaError> {
    let n = v.to_int()? as u32;
    Ok(((n & 0xFF) << 16) | (n & 0xFF00) | ((n >> 16) & 0xFF))
}

fn vba_color(rgb: Option<u32>) -> Value {
    let v = rgb.unwrap_or(0xFFFFFF);
    Value::Int(i64::from(((v & 0xFF) << 16) | (v & 0xFF00) | ((v >> 16) & 0xFF)))
}

/// Reads a control member. `form` is the form's name, for the events a read may fire (none do).
pub fn control_get(c: &mut ControlState, member: &str, args: &[HostArg]) -> Result<Value, VbaError> {
    let m = member.to_ascii_lowercase();
    Ok(match m.as_str() {
        "value" | "_default" => c.value(),
        "text" => match c.kind() {
            ControlKind::ListBox => c.list_row().and_then(|r| r.first().cloned()).map_or(Value::from(""), Value::from),
            _ => Value::from(c.text.clone()),
        },
        "caption" => Value::from(c.caption.clone()),
        "name" => Value::from(c.name().to_string()),
        "enabled" => Value::Bool(c.enabled),
        "visible" => Value::Bool(c.visible),
        "locked" => Value::Bool(c.locked),
        "left" => Value::Double(f64::from(c.left)),
        "top" => Value::Double(f64::from(c.top)),
        "width" => Value::Double(f64::from(c.width)),
        "height" => Value::Double(f64::from(c.height)),
        "backcolor" => vba_color(c.back),
        "forecolor" => vba_color(c.fore.or(Some(0))),
        "tag" => Value::from(c.tag.clone()),
        "tabindex" => Value::Int(i64::from(c.design.tab_index)),
        "listcount" => Value::Int(c.list.len() as i64),
        "listindex" => Value::Int(c.list_index),
        "columncount" => Value::Int(i64::from(c.column_count)),
        "columnwidths" => Value::from(c.column_widths.clone()),
        "min" => Value::Int(i64::from(c.design.min)),
        "max" => Value::Int(i64::from(c.design.max)),
        "selstart" => Value::Int(c.sel_start as i64),
        "sellength" => Value::Int(c.sel_length as i64),
        "seltext" => Value::from(c.text.chars().skip(c.sel_start).take(c.sel_length).collect::<String>()),
        "multiline" => Value::Bool(c.design.multiline),
        "rowsource" => Value::from(c.design.row_source.clone()),
        "controlsource" => Value::from(c.design.control_source.clone()),
        "list" => {
            // `List` alone is the whole list as a 2-D array; `List(r [, c])` one entry.
            match (index_arg(arg(args, 0, "pvargIndex"))?, index_arg(arg(args, 1, "pvargColumn"))?) {
                (Some(r), col) => {
                    let row = usize::try_from(r)
                        .ok()
                        .and_then(|r| c.list.get(r))
                        .ok_or(VbaError::runtime(381, "Could not get the List property. Invalid property array index."))?;
                    let col = usize::try_from(col.unwrap_or(0)).unwrap_or(0);
                    row.get(col).map_or(Value::Null, |s| Value::from(s.clone()))
                }
                (None, _) => {
                    let cols = c.list.iter().map(Vec::len).max().unwrap_or(1).max(c.column_count.max(1) as usize);
                    let mut arr = gridcraft_vba::Array::new(vec![(0, c.list.len()), (0, cols)])?;
                    for (r, row) in c.list.iter().enumerate() {
                        for (ci, s) in row.iter().enumerate() {
                            if let Some(off) = arr.offset(&[r as i64, ci as i64])
                                && let Some(slot) = arr.data.get_mut(off)
                            {
                                *slot = Value::from(s.clone());
                            }
                        }
                    }
                    Value::Array(std::sync::Arc::new(arr))
                }
            }
        }
        "column" => {
            // `Column(c [, r])`: the selected row's column by default.
            let col = usize::try_from(index_arg(arg(args, 0, "pvargColumn"))?.unwrap_or(0)).unwrap_or(0);
            let row = match index_arg(arg(args, 1, "pvargIndex"))? {
                Some(r) => usize::try_from(r).ok().and_then(|r| c.list.get(r)),
                None => c.list_row(),
            };
            row.and_then(|r| r.get(col)).map_or(Value::Null, |s| Value::from(s.clone()))
        }
        "selected" => {
            let i = usize::try_from(index_arg(arg(args, 0, "pvargIndex"))?.unwrap_or(0)).unwrap_or(usize::MAX);
            Value::Bool(c.selected.get(i).copied().unwrap_or(false) || (c.list_index >= 0 && i as i64 == c.list_index))
        }
        "font" => return Err(VbaError::runtime(438, "Font is set through Font.Size / Font.Bold on the control")),
        _ => return Err(not_supported(c.name(), member)),
    })
}

/// Methods and property writes on a control; returns events the change fires (e.g. `Change`).
pub fn control_call(form: &str, c: &mut ControlState, member: &str, args: &[HostArg], value: Option<Value>) -> Result<(Value, Vec<Event>), VbaError> {
    let m = member.to_ascii_lowercase();
    let name = c.name().to_string();
    let mut events = Vec::new();
    let change = |events: &mut Vec<Event>| events.push(ev(form, format!("{name}_Change"), vec![]));
    if let Some(v) = value {
        match m.as_str() {
            "value" | "_default" | "text" => match c.kind() {
                ControlKind::CheckBox | ControlKind::OptionButton | ControlKind::ToggleButton => {
                    let b = !matches!(v, Value::Null | Value::Empty) && v.to_bool()?;
                    if b != c.checked {
                        c.checked = b;
                        change(&mut events);
                        events.push(ev(form, format!("{name}_Click"), vec![]));
                    }
                }
                ControlKind::SpinButton | ControlKind::ScrollBar => {
                    let p = v.to_int()?.clamp(i64::from(c.design.min.min(c.design.max)), i64::from(c.design.max.max(c.design.min))) as i32;
                    if p != c.position {
                        c.position = p;
                        change(&mut events);
                    }
                }
                ControlKind::ListBox | ControlKind::ComboBox if m != "text" || c.kind() == ControlKind::ListBox => {
                    let t = if matches!(v, Value::Null) { String::new() } else { v.to_text()? };
                    let idx = c.list.iter().position(|r| r.first().is_some_and(|x| *x == t)).map_or(-1, |i| i as i64);
                    let text_changed = c.kind() == ControlKind::ComboBox && c.text != t;
                    if c.kind() == ControlKind::ComboBox {
                        c.text = t;
                    }
                    if idx != c.list_index || text_changed {
                        c.list_index = idx;
                        change(&mut events);
                    }
                }
                ControlKind::Label | ControlKind::CommandButton => c.caption = v.to_text()?,
                _ => {
                    let t = if matches!(v, Value::Null) { String::new() } else { v.to_text()? };
                    if t != c.text {
                        c.text = t;
                        c.sel_start = c.text.chars().count();
                        c.sel_length = 0;
                        change(&mut events);
                    }
                }
            },
            "caption" => c.caption = v.to_text()?,
            "enabled" => c.enabled = v.to_bool()?,
            "visible" => c.visible = v.to_bool()?,
            "locked" => c.locked = v.to_bool()?,
            "left" => c.left = v.to_num()? as f32,
            "top" => c.top = v.to_num()? as f32,
            "width" => c.width = v.to_num()? as f32,
            "height" => c.height = v.to_num()? as f32,
            "backcolor" => c.back = Some(rgb_of(&v)?),
            "forecolor" => c.fore = Some(rgb_of(&v)?),
            "tag" => c.tag = v.to_text()?,
            "columncount" => c.column_count = v.to_int()?.clamp(1, 100) as i32,
            "columnwidths" => c.column_widths = v.to_text()?,
            "min" => c.design.min = v.to_int()? as i32,
            "max" => c.design.max = v.to_int()? as i32,
            "selstart" => c.sel_start = usize::try_from(v.to_int()?).unwrap_or(0).min(c.text.chars().count()),
            "sellength" => c.sel_length = usize::try_from(v.to_int()?).unwrap_or(0),
            "tabindex" => c.design.tab_index = v.to_int()? as i32,
            "multiline" => c.design.multiline = v.to_bool()?,
            "listindex" => {
                let i = v.to_int()?;
                if i < -1 || i >= c.list.len() as i64 {
                    return Err(VbaError::runtime(380, "Could not set the ListIndex property. Invalid property value."));
                }
                if i != c.list_index {
                    c.list_index = i;
                    if c.kind() == ControlKind::ComboBox {
                        c.text = c.list_row().and_then(|r| r.first().cloned()).unwrap_or_default();
                    }
                    change(&mut events);
                    events.push(ev(form, format!("{name}_Click"), vec![]));
                }
            }
            "list" => {
                match (index_arg(arg(args, 0, "pvargIndex"))?, index_arg(arg(args, 1, "pvargColumn"))?) {
                    (Some(r), col) => {
                        let r = usize::try_from(r).map_err(|_| VbaError::runtime(381, "Invalid property array index"))?;
                        let col = usize::try_from(col.unwrap_or(0)).unwrap_or(0).min(99);
                        let row =
                            c.list.get_mut(r).ok_or(VbaError::runtime(381, "Could not set the List property. Invalid property array index."))?;
                        if row.len() <= col {
                            row.resize(col + 1, String::new());
                        }
                        if let Some(cell) = row.get_mut(col) {
                            *cell = if matches!(v, Value::Null | Value::Empty) { String::new() } else { v.to_text()? };
                        }
                    }
                    (None, _) => {
                        // `List = array`: one or two dimensions.
                        c.list = rows_of(&v)?;
                        c.list_index = -1;
                        c.selected.clear();
                    }
                }
            }
            "rowsource" => c.design.row_source = v.to_text()?,
            "controlsource" => c.design.control_source = v.to_text()?,
            "selected" => {
                let i = usize::try_from(index_arg(arg(args, 0, "pvargIndex"))?.unwrap_or(0)).unwrap_or(usize::MAX);
                if i < c.list.len() {
                    if c.selected.len() < c.list.len() {
                        c.selected.resize(c.list.len(), false);
                    }
                    if let Some(s) = c.selected.get_mut(i) {
                        *s = v.to_bool()?;
                    }
                    if v.to_bool()? {
                        c.list_index = i as i64;
                    }
                }
            }
            _ => return Err(not_supported(&name, member)),
        }
        return Ok((Value::Empty, events));
    }
    let r = match m.as_str() {
        "additem" => {
            let item = arg(args, 0, "pvargItem")
                .map(|v| if matches!(v, Value::Null) { Ok(String::new()) } else { v.to_text() })
                .transpose()?
                .unwrap_or_default();
            let at = index_arg(arg(args, 1, "pvargIndex"))?;
            let row = vec![item];
            match at.and_then(|i| usize::try_from(i).ok()) {
                Some(i) if i <= c.list.len() => c.list.insert(i, row),
                Some(_) => return Err(VbaError::runtime(5, "Invalid argument")),
                None => c.list.push(row),
            }
            if c.list.len() > 1_000_000 {
                return Err(VbaError::runtime(7, "Out of memory"));
            }
            Value::Empty
        }
        "removeitem" => {
            let i = usize::try_from(index_arg(arg(args, 0, "pvargIndex"))?.unwrap_or(-1)).map_err(|_| VbaError::runtime(5, "Invalid argument"))?;
            if i >= c.list.len() {
                return Err(VbaError::runtime(5, "Invalid argument"));
            }
            c.list.remove(i);
            if c.list_index >= c.list.len() as i64 {
                c.list_index = -1;
            }
            Value::Empty
        }
        "clear" => {
            c.list.clear();
            c.list_index = -1;
            c.selected.clear();
            if c.kind() == ControlKind::ComboBox {
                c.text.clear();
            }
            Value::Empty
        }
        "setfocus" | "dropdown" | "copy" | "paste" | "cut" | "move" | "zorder" | "repaint" => Value::Empty,
        _ => return control_get(c, member, args).map(|v| (v, events)),
    };
    Ok((r, events))
}

/// Rows of a list from a 1-D (one item per row) or 2-D array, or a single value.
pub fn rows_of(v: &Value) -> Result<Vec<Vec<String>>, VbaError> {
    let text = |x: &Value| if matches!(x, Value::Null | Value::Empty) { Ok(String::new()) } else { x.to_text() };
    match v {
        Value::Array(a) => match a.dims.as_slice() {
            [(_, n)] => a.data.iter().take(*n).map(|x| text(x).map(|t| vec![t])).collect(),
            [(r0, rows), (c0, cols)] => {
                let mut out = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let mut row = Vec::with_capacity(*cols);
                    for c in 0..*cols {
                        let off = a.offset(&[r0 + r as i64, c0 + c as i64]);
                        row.push(match off.and_then(|o| a.data.get(o)) {
                            Some(x) => text(x)?,
                            None => String::new(),
                        });
                    }
                    out.push(row);
                }
                Ok(out)
            }
            _ => Err(VbaError::runtime(13, "Type mismatch")),
        },
        Value::Empty => Ok(vec![]),
        other => Ok(vec![vec![text(other)?]]),
    }
}

/// A blank control design (tests build forms in code).
#[cfg(test)]
pub(crate) mod tests_support {
    pub(crate) fn control(name: &str, kind: gridcraft_vba::forms::ControlKind) -> gridcraft_vba::forms::ControlDesign {
        super::tests::test_control(name, kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_vba::forms::{ControlDesign, ControlKind};

    fn design(kind: ControlKind) -> FormDesign {
        let mut d = FormDesign { name: "F".into(), caption: "Form".into(), width: 200.0, height: 100.0, ..Default::default() };
        let mut c = test_control("C1", kind);
        c.tab_index = 0;
        d.controls.push(c);
        d
    }

    pub(crate) fn test_control(name: &str, kind: ControlKind) -> ControlDesign {
        ControlDesign {
            name: name.into(),
            kind,
            id: 1,
            left: 0.0,
            top: 0.0,
            width: 50.0,
            height: 20.0,
            tab_index: 0,
            visible: true,
            tab_stop: true,
            enabled: true,
            locked: false,
            multiline: false,
            word_wrap: true,
            caption: String::new(),
            value: String::new(),
            fore: None,
            back: None,
            transparent: false,
            font: None,
            align: 1,
            password_char: None,
            max_length: 0,
            column_count: 1,
            drop_list: false,
            row_source: String::new(),
            control_source: String::new(),
            min: 0,
            max: 100,
            position: 0,
            small_change: 1,
            default: false,
            cancel: false,
            tip: String::new(),
        }
    }

    fn a(v: Value) -> HostArg {
        HostArg { name: None, value: v }
    }

    #[test]
    fn text_box_changes_fire_change_once() {
        let mut f = FormState::new(&design(ControlKind::TextBox));
        let c = &mut f.controls[0];
        let (_, ev) = control_call("F", c, "Value", &[], Some(Value::from("12"))).unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].proc, "C1_Change");
        let (_, ev) = control_call("F", c, "Text", &[], Some(Value::from("12"))).unwrap();
        assert!(ev.is_empty(), "same text: no event");
        assert_eq!(control_get(c, "Value", &[]).unwrap(), Value::from("12"));
    }

    #[test]
    fn list_boxes() {
        let mut f = FormState::new(&design(ControlKind::ListBox));
        let c = &mut f.controls[0];
        for item in ["a", "b", "c"] {
            control_call("F", c, "AddItem", &[a(Value::from(item))], None).unwrap();
        }
        control_call("F", c, "List", &[a(Value::Int(1)), a(Value::Int(1))], Some(Value::from("b2"))).unwrap();
        assert_eq!(control_get(c, "ListCount", &[]).unwrap(), Value::Int(3));
        assert_eq!(control_get(c, "List", &[a(Value::Int(1)), a(Value::Int(1))]).unwrap(), Value::from("b2"));
        let (_, ev) = control_call("F", c, "ListIndex", &[], Some(Value::Int(1))).unwrap();
        assert_eq!(ev.iter().map(|e| e.proc.as_str()).collect::<Vec<_>>(), vec!["C1_Change", "C1_Click"]);
        assert_eq!(control_get(c, "Value", &[]).unwrap(), Value::from("b"));
        assert_eq!(control_get(c, "Column", &[a(Value::Int(1))]).unwrap(), Value::from("b2"));
        assert!(control_call("F", c, "ListIndex", &[], Some(Value::Int(9))).is_err());
        control_call("F", c, "RemoveItem", &[a(Value::Int(0))], None).unwrap();
        assert_eq!(c.list.len(), 2);
        control_call("F", c, "Clear", &[], None).unwrap();
        assert_eq!(control_get(c, "ListIndex", &[]).unwrap(), Value::Int(-1));
        assert_eq!(control_get(c, "Value", &[]).unwrap(), Value::Null);
    }
}
