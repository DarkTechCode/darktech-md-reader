fn main() {
    let src = "- пункт раз\n- пункт два\n  - вложенный\n\n1. первый\n2. второй\n";
    let doc = darktech_md::md::parser::parse(std::sync::Arc::from(src));
    for b in &doc.blocks {
        if let darktech_md::md::blocks::BlockKind::ListItem { indent, marker, spans, .. } = b {
            let text: String = spans.iter().map(|s| s.text.as_str()).collect();
            println!("indent={indent} marker={marker:?} text={text:?}");
        } else {
            println!("{b:?}");
        }
    }
}
