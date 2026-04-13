use xee_xpath::error::Error;

pub(crate) fn render_error(filename: &str, src: &str, e: Error) {
    let red = ariadne::Color::Red;
    let message = e.error.message().to_string();
    let detail = e.error.detail().map(|s| s.to_string());
    let note = e.error.note().to_string();

    let mut report = ariadne::Report::build(ariadne::ReportKind::Error, (filename, (0..0)))
        .with_code(e.error.code());
    if !message.is_empty() {
        report = report.with_message(&message);
    }

    if let Some(span) = e.span {
        // Use the detail string as the inline label if available, otherwise
        // fall back to the general message.
        let label_text = detail.as_deref().unwrap_or(&message).to_string();
        report = report.with_label(
            ariadne::Label::new((filename, span.range()))
                .with_message(label_text)
                .with_color(red),
        )
    }
    report
        .finish()
        .eprint((filename, ariadne::Source::from(src)))
        .unwrap();
    // When there is no span, print detail as a sub-note under the title.
    if e.span.is_none() {
        if let Some(detail) = &detail {
            println!("  detail: {}", detail);
        }
    }
    if !note.is_empty() {
        println!("{}", note);
    }
}

pub(crate) fn render_parse_error(src: &str, e: xot::ParseError) {
    let red = ariadne::Color::Red;
    let mut report = ariadne::Report::build(ariadne::ReportKind::Error, ("source", (0..0)));

    report = report.with_label(
        ariadne::Label::new(("source", e.span().range()))
            .with_message(e)
            .with_color(red),
    );

    report
        .finish()
        .eprint(("source", ariadne::Source::from(src)))
        .unwrap();
}
