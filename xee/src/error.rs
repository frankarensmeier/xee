use xee_xpath::error::Error;

pub(crate) fn render_error(filename: &str, src: &str, e: Error) {
    let red = ariadne::Color::Red;
    let message = e.error.message().to_string();
    let note = e.error.note().to_string();

    let mut report = ariadne::Report::build(ariadne::ReportKind::Error, (filename, (0..0)))
        .with_code(e.error.code());

    if let Some(span) = e.span {
        report = report.with_label(
            ariadne::Label::new((filename, span.range()))
                .with_message(e.error.message())
                .with_color(red),
        )
    }
    report
        .finish()
        .eprint((filename, ariadne::Source::from(src)))
        .unwrap();
    if e.span.is_none() && !message.is_empty() {
        println!("{}", message);
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
