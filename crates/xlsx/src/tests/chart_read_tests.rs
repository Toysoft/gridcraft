//! Reading charts as Excel saved them: PivotChart series and deleted automatic titles.

use gridcraft_model::{Chart, ChartKind, Workbook};

const NS: &str =
    r#"xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;

fn read(xml: &str) -> Chart {
    crate::chart::read_chart(&crate::xml::parse(xml.as_bytes()).unwrap()).unwrap()
}

fn str_cache(f: &str, pts: &[&str]) -> String {
    let p: String = pts.iter().enumerate().map(|(i, v)| format!(r#"<c:pt idx="{i}"><c:v>{v}</c:v></c:pt>"#)).collect();
    format!(r#"<c:strRef><c:f>{f}</c:f><c:strCache><c:ptCount val="{}"/>{p}</c:strCache></c:strRef>"#, pts.len())
}

/// A numeric cache of `count` points; `pts` are (idx, value), so gaps can be left out.
fn num_cache(f: &str, count: usize, pts: &[(usize, &str)]) -> String {
    let p: String = pts.iter().map(|(i, v)| format!(r#"<c:pt idx="{i}"><c:v>{v}</c:v></c:pt>"#)).collect();
    format!(r#"<c:numRef><c:f>{f}</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="{count}"/>{p}</c:numCache></c:numRef>"#)
}

fn ser(tx: &str, cat: &str, val: &str) -> String {
    format!("<c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:tx>{tx}</c:tx><c:cat>{cat}</c:cat><c:val>{val}</c:val></c:ser>")
}

fn chart_xml(pivot: bool, plot: &str, title: &str) -> String {
    let src = if pivot { "<c:pivotSource><c:name>[Book.xlsx]Graph!PivotTable1</c:name><c:fmtId val=\"0\"/></c:pivotSource>" } else { "" };
    format!(
        r#"<c:chartSpace {NS}>{src}<c:chart>{title}<c:plotArea>{plot}<c:catAx><c:axId val="1"/></c:catAx><c:valAx><c:axId val="2"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#
    )
}

#[test]
fn pivot_charts_plot_their_cached_points_when_the_references_do_not_cover_them() {
    // Excel points every formula of this combo PivotChart at the PivotTable's corner cell and
    // draws it from the PivotTable; the cached points are what it shows.
    let cats = ["Truck 1", "Van \"B\"", "Car"];
    let bar =
        ser(&str_cache("Graph!$J$2", &["Events"]), &str_cache("Graph!$J$2", &cats), &num_cache("Graph!$J$2", 3, &[(0, "18"), (1, "12"), (2, "5")]));
    // A point the cache leaves out stays a gap, so later points keep their positions.
    let line = ser(&str_cache("Graph!$J$2", &["Share"]), &str_cache("Graph!$J$2", &cats), &num_cache("Graph!$J$2", 3, &[(0, "5.3E-2"), (2, "0.5")]));
    let plot = format!(
        r#"<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/>{bar}<c:axId val="1"/><c:axId val="2"/></c:barChart><c:lineChart><c:grouping val="standard"/>{line}<c:axId val="1"/><c:axId val="2"/></c:lineChart>"#
    );
    let c = read(&chart_xml(true, &plot, ""));
    assert_eq!(c.pivot.as_deref(), Some("[Book.xlsx]Graph!PivotTable1"));
    assert_eq!(c.kind, ChartKind::Combo);
    let s = &c.series;
    assert_eq!(s[0].name.as_deref(), Some("\"Events\""));
    assert_eq!(s[0].categories.as_deref(), Some(r#"{"Truck 1","Van ""B""","Car"}"#));
    assert_eq!(s[0].values, "{18,12,5}");
    assert_eq!(s[1].name.as_deref(), Some("\"Share\""));
    assert_eq!(s[1].values, "{0.053,#N/A,0.5}");

    // The block Excel writes takes in the grand total row (4 cells for 3 points): the total is
    // not plotted.
    let totals = ser(
        &str_cache("Summary!$B$4", &["Total"]),
        &str_cache("Summary!$A$5:$A$8", &cats),
        &num_cache("Summary!$B$5:$B$8", 3, &[(0, "1010"), (1, "185.14"), (2, "120")]).replace("General", "\"$\"#,##0"),
    );
    let c = read(&chart_xml(true, &format!(r#"<c:pieChart>{totals}</c:pieChart>"#), ""));
    assert_eq!(c.series[0].values, "{1010,185.14,120}");
    assert_eq!(c.series[0].categories.as_deref(), Some(r#"{"Truck 1","Van ""B""","Car"}"#));
    // The cached points keep the cache's number format (data labels show "$185"), and so does
    // a save.
    assert_eq!(c.series[0].number_format.as_deref(), Some("\"$\"#,##0"));
    let back = read(&crate::chart::write_chart(&Workbook::new(), 0, &c));
    assert_eq!(back.series[0].number_format, c.series[0].number_format);

    // References that cover the cached points stay live, and so do an ordinary chart's.
    let live = ser(&str_cache("Summary!$B$4", &["Total"]), &str_cache("Summary!$A$5:$A$7", &cats), &num_cache("Summary!$B$5:$B$7", 3, &[(0, "1")]));
    let c = read(&chart_xml(true, &format!(r#"<c:pieChart>{live}</c:pieChart>"#), ""));
    assert_eq!((c.series[0].name.as_deref(), c.series[0].values.as_str()), (Some("Summary!$B$4"), "Summary!$B$5:$B$7"));
    assert_eq!(c.series[0].number_format, None, "live references use their cells' format");
    let plain = ser(&str_cache("Graph!$J$2", &["Events"]), &str_cache("Graph!$J$2", &cats), &num_cache("Graph!$J$2", 3, &[(0, "18")]));
    let c = read(&chart_xml(false, &format!(r#"<c:pieChart>{plain}</c:pieChart>"#), ""));
    assert_eq!(c.series[0].values, "Graph!$J$2");

    // Saved again, the cached points come back as they were: quotes, gaps and all. (A literal
    // name is saved as plain text, which names the series the same way.)
    let c = read(&chart_xml(true, &plot, ""));
    let back = read(&crate::chart::write_chart(&Workbook::new(), 0, &c));
    assert_eq!(back.pivot, c.pivot);
    let parts = |c: &Chart| {
        c.series
            .iter()
            .map(|s| (s.name.as_deref().map(|n| n.trim_matches('"').to_string()), s.categories.clone(), s.values.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(parts(&back), parts(&c));
}

#[test]
fn a_deleted_automatic_title_stays_deleted() {
    let one = ser("<c:v>Vehicles</c:v>", &str_cache("S!$A$1:$A$2", &["a", "b"]), &num_cache("S!$B$1:$B$2", 2, &[]));
    let pie = format!("<c:pieChart>{one}</c:pieChart>");
    // A lone series names an automatic title unless the file turned it off.
    assert_eq!(read(&chart_xml(false, &pie, "")).title, None);
    assert_eq!(read(&chart_xml(false, &pie, r#"<c:autoTitleDeleted val="0"/>"#)).title, None);
    let deleted = read(&chart_xml(false, &pie, r#"<c:autoTitleDeleted val="1"/>"#));
    assert_eq!(deleted.title.as_deref(), Some(""));
    // An explicit title wins over the flag.
    let t = r#"<c:title><c:tx><c:rich><a:p><a:r><a:t>Risk</a:t></a:r></a:p></c:rich></c:tx></c:title><c:autoTitleDeleted val="1"/>"#;
    assert_eq!(read(&chart_xml(false, &pie, t)).title.as_deref(), Some("Risk"));

    // Both states survive a save.
    let wb = Workbook::new();
    assert_eq!(read(&crate::chart::write_chart(&wb, 0, &deleted)).title.as_deref(), Some(""));
    let auto = read(&chart_xml(false, &pie, ""));
    assert_eq!(read(&crate::chart::write_chart(&wb, 0, &auto)).title, None);
}
