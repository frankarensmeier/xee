use xee_interpreter::interpreter::Program;
use xee_xpath::error::Error;

pub(crate) fn render_error(filename: &str, src: &str, e: Error) {
    render_error_with_span(filename, src, e.span.map(|span| span.range()), e);
}

pub(crate) fn render_program_error(
    program: &Program,
    fallback_filename: &str,
    fallback_src: &str,
    e: Error,
) {
    if let Some(span) = e.span {
        if let Some((uri, local_span)) = program.resolve_source_span(span) {
            let source = program
                .source_for_uri(&uri)
                .map(|value| value.to_string())
                .or_else(|| read_source_from_uri(&uri));
            if let Some(source) = source {
                let filename = uri_to_display_name(&uri);
                render_error_with_span(&filename, &source, Some(local_span), e);
                return;
            }
        }
    }

    render_error_with_span(
        fallback_filename,
        fallback_src,
        e.span.map(|span| span.range()),
        e,
    );
}

fn render_error_with_span(
    filename: &str,
    src: &str,
    span: Option<std::ops::Range<usize>>,
    e: Error,
) {
    let red = ariadne::Color::Red;
    let message = e.error.message().to_string();
    let detail = e.detail().map(|s| s.to_string());
    let note = e.error.note().to_string();

    let mut report = ariadne::Report::build(ariadne::ReportKind::Error, (filename, (0..0)))
        .with_code(e.error.code());
    if !message.is_empty() {
        report = report.with_message(&message);
    }

    if let Some(label_span) = span.clone() {
        // Use the detail string as the inline label if available, otherwise
        // fall back to the general message.
        let label_text = detail.as_deref().unwrap_or(&message).to_string();
        report = report.with_label(
            ariadne::Label::new((filename, label_span))
                .with_message(label_text)
                .with_color(red),
        )
    }
    report
        .finish()
        .eprint((filename, ariadne::Source::from(src)))
        .unwrap();
    // When there is no span, print detail as a sub-note under the title.
    if span.is_none() {
        if let Some(detail) = &detail {
            println!("  detail: {}", detail);
        }
    }
    if !note.is_empty() {
        println!("{}", note);
    }
}

fn read_source_from_uri(uri: &str) -> Option<String> {
    let path = uri.strip_prefix("file://")?.replace("%20", " ");
    std::fs::read_to_string(path).ok()
}

fn uri_to_display_name(uri: &str) -> String {
    uri.strip_prefix("file://")
        .map(|path| path.replace("%20", " "))
        .unwrap_or_else(|| uri.to_string())
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
