//! Terminal output. Colors switch off automatically when stdout is not a terminal.

use comfy_table::presets::UTF8_FULL_CONDENSED;
use comfy_table::{CellAlignment, Table};
use console::style;

use crate::markets::Candidate;
use crate::pipeline::Assessment;
use crate::research::Brief;
use crate::signal::Verdict;

pub fn price(x: Option<f64>) -> String {
    x.map_or_else(|| "-".into(), |v| format!("{v:.3}"))
}

/// `12345.6` -> `"12,346"`.
pub fn thousands(x: f64) -> String {
    let digits = format!("{:.0}", x.abs());
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if x < -0.5 {
        out.push('-');
    }
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Print `title` and return an empty table; columns from `left_aligned` on are right-aligned.
pub fn table(title: &str, headers: &[&str], left_aligned: usize) -> Table {
    println!("{}", style(title).bold());
    let mut t = Table::new();
    t.load_preset(UTF8_FULL_CONDENSED).set_header(headers.iter().copied());
    for i in left_aligned..headers.len() {
        if let Some(col) = t.column_mut(i) {
            col.set_cell_alignment(CellAlignment::Right);
        }
    }
    t
}

pub fn rule(title: &str) {
    let width = usize::from(console::Term::stdout().size().1).clamp(40, 120);
    let fill = width.saturating_sub(title.chars().count() + 4);
    println!("{} {} {}", style("──").dim(), style(title).bold(), style("─".repeat(fill)).dim());
}

pub fn print_brief(b: &Brief, cached: bool, full: bool) {
    let tag = if cached { "cached".to_owned() } else { format!("${:.4}", b.cost) };
    let as_of = if b.as_of.is_empty() { "?" } else { &b.as_of };
    let model = if b.model.is_empty() { "researcher" } else { &b.model };
    println!("  {} as of {as_of} ({model}, {tag}): {}", style("evidence").cyan(), b.summary);
    if !full {
        return;
    }
    for fact in &b.key_facts {
        println!("    • {fact}");
    }
    if !b.latest_development.is_empty() {
        println!("    latest: {}", b.latest_development);
    }
    for event in &b.scheduled_events {
        println!("    {} {event}", style("scheduled:").cyan());
    }
    if !b.resolution_source_status.is_empty() {
        println!("    resolution source: {}", b.resolution_source_status);
    }
    if !b.for_yes.is_empty() {
        println!("    {} {}", style("for YES:").green(), b.for_yes.join(" | "));
    }
    if !b.against_yes.is_empty() {
        println!("    {} {}", style("against YES:").red(), b.against_yes.join(" | "));
    }
    for url in b.sources.iter().take(8) {
        println!("    {}", style(url).dim());
    }
}

pub fn print_unclear(c: &Candidate, reason: &str) {
    println!("{}", style(format!("{}: skip: {reason}", c.market.slug)).dim());
}

pub fn print_decision(c: &Candidate, a: &Assessment, verdict: &Verdict) {
    let (m, book, v) = (&c.market, &c.book, &a.view);
    let days = m.days_to_resolution().map_or_else(|| "?".into(), |d| d.to_string());
    println!("{}", style(&m.slug).bold());
    println!("  {}", m.question);
    println!(
        "  market yes bid/ask {}/{}  no ask {}  days={days}",
        price(book.yes_bid),
        price(book.yes_ask),
        price(book.no_ask)
    );
    println!(
        "  Jev: p_yes={:.2} answerable={:.2} clarity={} (conf {}) cost=${:.6}",
        v.p_yes,
        v.answerable,
        v.clarity_mean.map_or_else(|| "?".into(), |x| format!("{x:.2}")),
        v.clarity_confidence.map_or_else(|| "?".into(), |x| format!("{x:.2}")),
        v.cost
    );
    if let Some(b) = &a.brief {
        print_brief(b, a.cached, false);
    }
    match verdict {
        Verdict::Trade(t) => println!(
            "  {} BUY {} {} @ {} (${:.2}) edge {:+.2}",
            style("TRADE").green().bold(),
            t.outcome,
            t.size,
            t.price,
            t.usd,
            t.edge
        ),
        Verdict::Skip(why) => println!("  {}", style(format!("skip: {}", why.reason)).dim()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_thousands() {
        assert_eq!(thousands(0.0), "0");
        assert_eq!(thousands(999.4), "999");
        assert_eq!(thousands(1234.0), "1,234");
        assert_eq!(thousands(1234567.8), "1,234,568");
        assert_eq!(thousands(-2500.0), "-2,500");
    }
}
