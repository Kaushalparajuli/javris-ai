//! Free primary sources Jarvis can read directly: SEC EDGAR filings, research papers from
//! Semantic Scholar (with arXiv as the fallback), and YouTube videos through Gemini. None of them
//! need a sign-in. Everything they return is written by other people, so the voice tools hand it
//! to Gemini as information, never instructions.

use crate::settings;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// The SEC asks every caller to name itself with a contact address; without it requests get 403.
const SEC_AGENT: &str = "Jarvis Research jarvis@fikraventures.co";
const SEC_GAP: Duration = Duration::from_millis(110); // the SEC allows 10 requests a second
const ARXIV_GAP: Duration = Duration::from_secs(3); // arXiv asks for one request every 3 seconds
const READ_CAP: usize = 60_000;
const WINDOWS: usize = 8;
const WINDOW: usize = 1_500;

// ---------- shared plumbing ----------

static SEC_LAST: tokio::sync::Mutex<Option<Instant>> = tokio::sync::Mutex::const_new(None);
static ARXIV_LAST: tokio::sync::Mutex<Option<Instant>> = tokio::sync::Mutex::const_new(None);

/// Waits until at least `gap` has passed since the last request to the same service.
async fn pace(last: &tokio::sync::Mutex<Option<Instant>>, gap: Duration) {
    let mut at = last.lock().await;
    if let Some(t) = *at {
        let since = t.elapsed();
        if since < gap {
            tokio::time::sleep(gap - since).await;
        }
    }
    *at = Some(Instant::now());
}

fn client(secs: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder().timeout(Duration::from_secs(secs)).build().map_err(|e| e.to_string())
}

/// A client for the SEC that only follows redirects staying on sec.gov, so sec_read can't be bounced elsewhere.
fn sec_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(40))
        .redirect(reqwest::redirect::Policy::custom(|a| {
            let host = a.url().host_str().unwrap_or("");
            if a.previous().len() < 5 && a.url().scheme() == "https" && (host == "sec.gov" || host.ends_with(".sec.gov")) {
                a.follow()
            } else {
                a.stop()
            }
        }))
        .build()
        .map_err(|e| e.to_string())
}

/// GET a page from the SEC, paced and with the contact header the SEC requires.
async fn sec_get(url: &str, query: &[(&str, String)]) -> Result<reqwest::Response, String> {
    pace(&SEC_LAST, SEC_GAP).await;
    let res = sec_client()?
        .get(url)
        .query(query)
        .header("User-Agent", SEC_AGENT)
        .send()
        .await
        .map_err(|e| format!("Couldn't reach the SEC: {e}"))?;
    let status = res.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err("The SEC has no such page.".into());
    }
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err("The SEC is limiting requests right now. Try again in a minute.".into());
    }
    if !status.is_success() {
        return Err(format!("The SEC request failed ({status})."));
    }
    Ok(res)
}

async fn sec_json(url: &str, query: &[(&str, String)]) -> Result<Value, String> {
    sec_get(url, query).await?.json().await.map_err(|e| format!("The SEC sent something unreadable: {e}"))
}

fn chars(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    format!("{}…", s.chars().take(n).collect::<String>().trim_end())
}

// ---------- SEC: companies ----------

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Company {
    cik: String,
    ticker: String,
    name: String,
}

static TICKERS: Mutex<Option<Arc<Vec<Company>>>> = Mutex::new(None);

/// The SEC's ticker list, in its own order (roughly by market value).
fn parse_tickers(v: &Value) -> Vec<Company> {
    let Some(map) = v.as_object() else { return vec![] };
    let mut rows: Vec<(u64, Company)> = map
        .iter()
        .filter_map(|(k, r)| {
            let cik = r["cik_str"].as_u64().or_else(|| r["cik_str"].as_str()?.parse().ok())?;
            Some((
                k.parse().unwrap_or(u64::MAX),
                Company { cik: format!("{cik:010}"), ticker: r["ticker"].as_str().unwrap_or("").to_string(), name: r["title"].as_str().unwrap_or("").to_string() },
            ))
        })
        .collect();
    rows.sort_by_key(|(k, _)| *k);
    rows.into_iter().map(|(_, c)| c).collect()
}

/// Letters and digits only, lower case, so "Apple Inc." matches "APPLE INC".
fn squash(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Find a company by ticker, CIK or name: exact ticker first, then the name.
fn find_company(list: &[Company], query: &str) -> Option<Company> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    if q.chars().all(|c| c.is_ascii_digit()) {
        let cik = format!("{:010}", q.parse::<u64>().ok()?);
        return list.iter().find(|c| c.cik == cik).cloned();
    }
    let tick = q.trim_start_matches('$').to_uppercase().replace('.', "-");
    if let Some(c) = list.iter().find(|c| c.ticker.to_uppercase() == tick) {
        return Some(c.clone());
    }
    let want = squash(q);
    if want.is_empty() {
        return None;
    }
    let names: Vec<String> = list.iter().map(|c| squash(&c.name)).collect();
    let pick = |f: &dyn Fn(&str) -> bool| names.iter().position(|n| f(n)).map(|i| list[i].clone());
    pick(&|n| n == want)
        .or_else(|| pick(&|n| n.split(' ').filter(|w| !["inc", "corp", "co", "ltd", "plc", "corporation", "company", "the", "holdings", "group"].contains(w)).collect::<Vec<_>>().join(" ") == want))
        .or_else(|| pick(&|n| n.starts_with(&format!("{want} "))))
        .or_else(|| pick(&|n| n.contains(&want)))
}

async fn tickers() -> Result<Arc<Vec<Company>>, String> {
    if let Some(t) = TICKERS.lock().unwrap().clone() {
        return Ok(t);
    }
    let v = sec_json("https://www.sec.gov/files/company_tickers.json", &[]).await?;
    let list = Arc::new(parse_tickers(&v));
    if list.is_empty() {
        return Err("The SEC's company list came back empty. Try again later.".into());
    }
    *TICKERS.lock().unwrap() = Some(list.clone());
    Ok(list)
}

/// Look up a public company by ticker or name. Returns its 10-digit SEC number (CIK), ticker and name.
#[tauri::command]
pub async fn sec_company(query: String) -> Result<Company, String> {
    if query.trim().is_empty() {
        return Err("Say which company to look up.".into());
    }
    let list = tickers().await?;
    find_company(&list, &query).ok_or_else(|| format!("No SEC-listed company matches \"{}\". Try its stock ticker.", query.trim()))
}

// ---------- SEC: filings ----------

#[derive(Serialize, Debug, PartialEq)]
pub struct Filing {
    form: String,
    date: String,
    description: String,
    url: String,
}

#[derive(Serialize)]
pub struct Filings {
    company: Company,
    filings: Vec<Filing>,
}

fn doc_url(cik: &str, accession: &str, file: &str) -> String {
    let cik: u64 = cik.trim().parse().unwrap_or(0);
    format!("https://www.sec.gov/Archives/edgar/data/{cik}/{}/{file}", accession.replace('-', ""))
}

/// The newest filings in `filings.recent`, which the SEC stores as parallel columns.
fn parse_recent(v: &Value, cik: &str, forms: &[String], limit: usize) -> Vec<Filing> {
    let r = &v["filings"]["recent"];
    let col = |k: &str, i: usize| r[k][i].as_str().unwrap_or("").to_string();
    let n = r["accessionNumber"].as_array().map(|a| a.len()).unwrap_or(0);
    let want: Vec<String> = forms.iter().map(|f| f.trim().to_uppercase()).filter(|f| !f.is_empty()).collect();
    let mut out = vec![];
    for i in 0..n {
        let form = col("form", i);
        if !want.is_empty() && !want.contains(&form.to_uppercase()) {
            continue;
        }
        let file = col("primaryDocument", i);
        let acc = col("accessionNumber", i);
        let url = if file.is_empty() {
            format!("https://www.sec.gov/Archives/edgar/data/{}/{}/", cik.parse::<u64>().unwrap_or(0), acc.replace('-', ""))
        } else {
            doc_url(cik, &acc, &file)
        };
        let desc = col("primaryDocDescription", i);
        out.push(Filing { description: if desc.is_empty() { form.clone() } else { desc }, form, date: col("filingDate", i), url });
    }
    out.sort_by(|a, b| b.date.cmp(&a.date));
    out.truncate(limit);
    out
}

/// A company's newest SEC filings, optionally only some forms (10-K, 10-Q, 8-K…), with links to each document.
#[tauri::command]
pub async fn sec_filings(company: String, forms: Option<Vec<String>>, limit: Option<usize>) -> Result<Filings, String> {
    let c = sec_company(company).await?;
    let v = sec_json(&format!("https://data.sec.gov/submissions/CIK{}.json", c.cik), &[]).await?;
    let forms = forms.unwrap_or_default();
    let filings = parse_recent(&v, &c.cik, &forms, limit.unwrap_or(10).clamp(1, 40));
    if filings.is_empty() && !forms.is_empty() {
        return Err(format!("{} has no recent {} filings.", c.name, forms.join(" or ")));
    }
    Ok(Filings { company: c, filings })
}

// ---------- SEC: full-text search ----------

#[derive(Serialize, Debug, PartialEq)]
pub struct Hit {
    company: String,
    form: String,
    date: String,
    url: String,
}

/// Hits from EDGAR full-text search. The endpoint is undocumented, so anything odd is skipped.
fn parse_hits(v: &Value, max: usize) -> Vec<Hit> {
    let mut out = vec![];
    for h in v["hits"]["hits"].as_array().into_iter().flatten() {
        let s = &h["_source"];
        let id = h["_id"].as_str().unwrap_or("");
        let (id_acc, file) = id.split_once(':').unwrap_or((id, ""));
        let acc = s["adsh"].as_str().filter(|a| !a.is_empty()).unwrap_or(id_acc);
        let cik = s["ciks"][0].as_str().map(String::from).or_else(|| s["ciks"][0].as_u64().map(|n| n.to_string())).unwrap_or_default();
        if acc.is_empty() || cik.is_empty() {
            continue;
        }
        let url = if file.is_empty() { doc_url(&cik, acc, "") } else { doc_url(&cik, acc, file) };
        if out.iter().any(|x: &Hit| x.url == url) {
            continue;
        }
        let company = one_line(s["display_names"][0].as_str().unwrap_or(""));
        let form = s["form"].as_str().or_else(|| s["root_forms"][0].as_str()).unwrap_or("").to_string();
        out.push(Hit { company, form, date: s["file_date"].as_str().unwrap_or("").to_string(), url });
        if out.len() == max {
            break;
        }
    }
    out
}

/// Search the full text of SEC filings since 2001. Put a phrase in quotes to match it exactly.
#[tauri::command]
pub async fn sec_search(query: String, forms: Option<Vec<String>>, from_date: Option<String>, to_date: Option<String>) -> Result<Vec<Hit>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Err("Say what to search the filings for.".into());
    }
    let mut params: Vec<(&str, String)> = vec![("q", q.to_string())];
    let forms: Vec<String> = forms.unwrap_or_default().into_iter().map(|f| f.trim().to_uppercase()).filter(|f| !f.is_empty()).collect();
    if !forms.is_empty() {
        params.push(("forms", forms.join(",")));
    }
    let day = |d: Option<String>| d.map(|s| s.trim().to_string()).filter(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok());
    let (from, to) = (day(from_date), day(to_date));
    if from.is_some() || to.is_some() {
        params.push(("dateRange", "custom".into()));
        params.push(("startdt", from.unwrap_or_else(|| "2001-01-01".into())));
        params.push(("enddt", to.unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string())));
    }
    let v = sec_json("https://efts.sec.gov/LATEST/search-index", &params).await?;
    Ok(parse_hits(&v, 10))
}

// ---------- SEC: reading a document ----------

#[derive(Serialize)]
pub struct Document {
    url: String,
    text: String,
    truncated: bool,
    /// Only when a query was given: how many passages matched.
    #[serde(skip_serializing_if = "Option::is_none")]
    passages: Option<usize>,
}

/// Only documents in the SEC's filing archive may be read.
fn archive_url(url: &str) -> Result<reqwest::Url, String> {
    let u = reqwest::Url::parse(url.trim()).map_err(|_| "That isn't a web address.".to_string())?;
    let ok = u.scheme() == "https" && u.host_str() == Some("www.sec.gov") && u.port().is_none() && u.path().starts_with("/Archives/") && !u.path().contains("..");
    if !ok {
        return Err("sec_read only opens SEC filing documents (https://www.sec.gov/Archives/…). Use the links from sec_filings or sec_search.".into());
    }
    Ok(u)
}

fn entity(name: &str) -> Option<char> {
    if let Some(n) = name.strip_prefix('#') {
        let code = match n.strip_prefix(['x', 'X']) {
            Some(h) => u32::from_str_radix(h, 16).ok()?,
            None => n.parse().ok()?,
        };
        return char::from_u32(code).map(|c| if c == '\u{a0}' { ' ' } else { c });
    }
    Some(match name {
        "nbsp" | "ensp" | "emsp" | "thinsp" => ' ',
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "rsquo" | "lsquo" => '\'',
        "rdquo" | "ldquo" => '"',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "bull" | "middot" => '•',
        "sect" => '§',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "cent" => '¢',
        "pound" => '£',
        "euro" => '€',
        "deg" => '°',
        "frac12" => '½',
        _ => return None,
    })
}

/// Replace `&amp;`, `&#8217;` and friends with the characters they stand for.
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        match tail.find(';').filter(|&j| j > 0 && j <= 10).and_then(|j| entity(&tail[..j]).map(|c| (c, j))) {
            Some((c, j)) => {
                out.push(c);
                rest = &tail[j + 1..];
            }
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Readable text from an HTML filing: no scripts, styles or hidden XBRL data, one line per block.
fn html_to_text(html: &str) -> String {
    const BLOCKS: [&str; 16] = ["p", "div", "br", "tr", "li", "h1", "h2", "h3", "h4", "h5", "h6", "table", "ul", "ol", "hr", "title"];
    const SKIP: [&str; 4] = ["script", "style", "head", "ix:header"];
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() / 3);
    let mut i = 0;
    while i < html.len() {
        let Some(off) = html[i..].find('<') else {
            out.push_str(&html[i..]);
            break;
        };
        out.push_str(&html[i..i + off]);
        let at = i + off;
        let Some(end) = html[at..].find('>').map(|e| at + e) else { break };
        let tag = &lower[at + 1..end];
        let closing = tag.starts_with('/');
        let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric() || *c == ':' || *c == '-').collect();
        i = end + 1;
        if tag.starts_with("!--") {
            // Comments can contain '>', so skip to the real end.
            i = lower[at..].find("-->").map(|e| at + e + 3).unwrap_or(html.len());
            continue;
        }
        if !closing && SKIP.contains(&name.as_str()) && !tag.ends_with('/') {
            let close = format!("</{name}");
            i = lower[i..].find(&close).map(|e| i + e).and_then(|s| lower[s..].find('>').map(|e| s + e + 1)).unwrap_or(html.len());
            continue;
        }
        if BLOCKS.contains(&name.as_str()) {
            out.push('\n');
        } else if name == "td" || name == "th" {
            out.push(' ');
        }
    }
    let text = decode_entities(&out);
    let mut lines: Vec<String> = vec![];
    for l in text.lines() {
        let l = l.split_whitespace().collect::<Vec<_>>().join(" ");
        if l.is_empty() {
            if lines.last().is_some_and(|p| !p.is_empty()) {
                lines.push(String::new());
            }
        } else {
            lines.push(l);
        }
    }
    lines.join("\n").trim().to_string()
}

fn floor_char(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// The passages around the query words: up to `count` windows of about `size` characters, best
/// matches first by how many different words they hold, then shown in document order.
fn passages(text: &str, query: &str, count: usize, size: usize) -> Vec<String> {
    const STOP: [&str; 14] = ["the", "and", "for", "with", "what", "does", "did", "how", "are", "was", "this", "that", "from", "about"];
    let lower = text.to_ascii_lowercase(); // same byte offsets as `text`
    let mut words: Vec<String> = query.to_ascii_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() >= 3 && !STOP.contains(w)).map(String::from).collect();
    words.dedup();
    if words.is_empty() {
        words = query.to_ascii_lowercase().split_whitespace().map(String::from).collect();
    }
    let phrase = query.trim().trim_matches('"').to_ascii_lowercase();
    let hits: Vec<Vec<usize>> = words.iter().map(|w| lower.match_indices(w.as_str()).map(|(i, _)| i).collect()).collect();
    let in_range = |v: &Vec<usize>, a: usize, b: usize| v.partition_point(|&x| x < b) - v.partition_point(|&x| x < a);
    let mut cands: Vec<(usize, usize, usize, usize)> = vec![]; // (start, end, distinct, total)
    for &p in hits.iter().flatten() {
        let a = floor_char(text, p.saturating_sub(size / 2));
        let b = floor_char(text, a + size);
        let counts: Vec<usize> = hits.iter().map(|v| in_range(v, a, b)).collect();
        let mut distinct = counts.iter().filter(|&&n| n > 0).count();
        if phrase.contains(' ') && lower[a..b].contains(&phrase) {
            distinct += words.len(); // the exact phrase beats scattered words
        }
        cands.push((a, b, distinct, counts.iter().sum()));
    }
    cands.sort_by(|x, y| y.2.cmp(&x.2).then(y.3.cmp(&x.3)).then(x.0.cmp(&y.0)));
    let mut chosen: Vec<(usize, usize)> = vec![];
    for (a, b, _, _) in cands {
        if chosen.iter().any(|&(c, d)| a < d && c < b) {
            continue;
        }
        chosen.push((a, b));
        if chosen.len() == count {
            break;
        }
    }
    chosen.sort();
    chosen
        .into_iter()
        .map(|(a, b)| {
            // Drop the words cut in half at either edge.
            let mut words: Vec<&str> = text[a..b].split_whitespace().collect();
            if b < text.len() && !text[b..].starts_with(char::is_whitespace) && words.len() > 1 {
                words.pop();
            }
            if a > 0 && !text[..a].ends_with(char::is_whitespace) && words.len() > 1 {
                words.remove(0);
            }
            let s = words.join(" ");
            format!("{}{s}{}", if a > 0 { "…" } else { "" }, if b < text.len() { "…" } else { "" })
        })
        .collect()
}

/// Read an SEC filing document as text. With `query`, only the passages that mention it come back,
/// so a 200-page 10-K can be searched without reading all of it.
#[tauri::command]
pub async fn sec_read(url: String, query: Option<String>) -> Result<Document, String> {
    let u = archive_url(&url)?;
    let res = sec_get(u.as_str(), &[]).await?;
    let html = res.text().await.map_err(|e| format!("Couldn't read that filing: {e}"))?;
    let looks_html = html.trim_start().starts_with('<') || html.contains("</");
    let text = if looks_html { html_to_text(&html) } else { html.trim().to_string() };
    if text.is_empty() {
        return Err("That filing has no readable text.".into());
    }
    if let Some(q) = query.map(|q| q.trim().to_string()).filter(|q| !q.is_empty()) {
        let found = passages(&text, &q, WINDOWS, WINDOW);
        let n = found.len();
        return Ok(Document {
            url: u.to_string(),
            text: if n == 0 { format!("Nothing in this document mentions \"{q}\".") } else { found.join("\n\n") },
            truncated: false,
            passages: Some(n),
        });
    }
    let truncated = text.chars().count() > READ_CAP;
    Ok(Document { url: u.to_string(), text: if truncated { chars(&text, READ_CAP) } else { text }, truncated, passages: None })
}

// ---------- papers ----------

#[derive(Serialize, Debug, PartialEq)]
pub struct Paper {
    title: String,
    year: Option<i64>,
    authors: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    citations: Option<i64>,
    summary: String,
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pdf: Option<String>,
}

#[derive(Serialize)]
pub struct Papers {
    source: String,
    papers: Vec<Paper>,
}

fn author_line(names: &[String]) -> String {
    match names.len() {
        0 => String::new(),
        1..=3 => names.join(", "),
        _ => format!("{} et al.", names[..3].join(", ")),
    }
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_s2(v: &Value) -> Vec<Paper> {
    v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let title = one_line(p["title"].as_str()?);
            let names: Vec<String> = p["authors"].as_array().into_iter().flatten().filter_map(|a| a["name"].as_str().map(String::from)).collect();
            let summary = p["tldr"]["text"].as_str().filter(|t| !t.is_empty()).map(one_line).unwrap_or_else(|| chars(&one_line(p["abstract"].as_str().unwrap_or("")), 300));
            let url = p["url"].as_str().map(String::from).or_else(|| p["externalIds"]["DOI"].as_str().map(|d| format!("https://doi.org/{d}"))).unwrap_or_default();
            let pdf = p["openAccessPdf"]["url"].as_str().filter(|u| !u.is_empty()).map(String::from).or_else(|| p["externalIds"]["ArXiv"].as_str().map(|a| format!("https://arxiv.org/pdf/{a}")));
            Some(Paper { title, year: p["year"].as_i64(), authors: author_line(&names), citations: p["citationCount"].as_i64(), summary, url, pdf })
        })
        .collect()
}

/// The text inside the first `<tag ...>…</tag>` in `s`.
fn xml_tag<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let mut from = 0;
    loop {
        let at = from + s[from..].find(&open)?;
        let after = &s[at + open.len()..];
        // Make sure it's this tag and not a longer one (e.g. <author> vs <authority>).
        if after.starts_with('>') || after.starts_with(' ') {
            let start = at + open.len() + after.find('>')? + 1;
            let end = start + s[start..].find(&format!("</{tag}>"))?;
            return Some(&s[start..end]);
        }
        from = at + open.len();
    }
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let i = tag.find(&key)? + key.len();
    Some(&tag[i..i + tag[i..].find('"')?])
}

fn parse_arxiv(xml: &str) -> Vec<Paper> {
    xml.split("<entry>")
        .skip(1)
        .filter_map(|e| {
            let e = e.split("</entry>").next().unwrap_or(e);
            let title = one_line(&decode_entities(xml_tag(e, "title")?));
            let id = xml_tag(e, "id").unwrap_or("").trim().to_string();
            let names: Vec<String> = e.split("<author>").skip(1).filter_map(|a| xml_tag(a, "name")).map(|n| one_line(&decode_entities(n))).collect();
            let year = xml_tag(e, "published").and_then(|d| d.trim().get(..4)).and_then(|y| y.parse().ok());
            let summary = chars(&one_line(&decode_entities(xml_tag(e, "summary").unwrap_or(""))), 300);
            let pdf = e.split("<link").skip(1).map(|l| &l[..l.find('>').unwrap_or(l.len())]).find(|l| attr(l, "title") == Some("pdf")).and_then(|l| attr(l, "href")).map(String::from);
            Some(Paper { title, year, authors: author_line(&names), citations: None, summary, url: id, pdf })
        })
        .collect()
}

/// "2024" or "2020-2024" as a pair of years.
fn year_span(year: &str) -> Option<(i32, i32)> {
    let y = year.trim();
    let (a, b) = y.split_once('-').unwrap_or((y, y));
    let (a, b) = (a.trim().parse().ok().unwrap_or(1900), b.trim().parse().ok().unwrap_or(2100));
    (!y.is_empty() && a <= b).then_some((a, b))
}

async fn semantic_scholar(query: &str, year: Option<&str>, limit: usize) -> Result<Vec<Paper>, String> {
    let http = client(30)?;
    let key = std::env::var("SEMANTIC_SCHOLAR_API_KEY").unwrap_or_default();
    let mut params = vec![("query", query.to_string()), ("limit", limit.to_string()), ("fields", "title,abstract,year,authors,citationCount,url,openAccessPdf,tldr,externalIds".to_string())];
    if let Some(y) = year.filter(|y| !y.trim().is_empty()) {
        params.push(("year", y.trim().to_string()));
    }
    for attempt in 0..2 {
        let mut req = http.get("https://api.semanticscholar.org/graph/v1/paper/search").query(&params);
        if !key.is_empty() {
            req = req.header("x-api-key", &key);
        }
        let res = req.send().await.map_err(|e| format!("Couldn't reach Semantic Scholar: {e}"))?;
        let status = res.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt == 0 {
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }
        if !status.is_success() {
            return Err(format!("Semantic Scholar failed ({status})."));
        }
        let v: Value = res.json().await.map_err(|e| e.to_string())?;
        return Ok(parse_s2(&v));
    }
    Err("Semantic Scholar is busy.".into())
}

async fn arxiv(query: &str, year: Option<&str>, limit: usize) -> Result<Vec<Paper>, String> {
    let mut q = format!("all:\"{}\"", query.replace('"', ""));
    if let Some((a, b)) = year.and_then(year_span) {
        q.push_str(&format!(" AND submittedDate:[{a}01010000 TO {b}12312359]"));
    }
    pace(&ARXIV_LAST, ARXIV_GAP).await;
    let res = client(40)?
        .get("https://export.arxiv.org/api/query")
        .query(&[("search_query", q), ("max_results", limit.to_string()), ("sortBy", "relevance".into())])
        .send()
        .await
        .map_err(|e| format!("Couldn't reach arXiv: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("arXiv failed ({}).", res.status()));
    }
    Ok(parse_arxiv(&res.text().await.map_err(|e| e.to_string())?))
}

/// Find research papers: Semantic Scholar first (citations and one-line summaries), arXiv if that
/// is busy or finds nothing. `year` is "2024" or a range like "2020-2024".
#[tauri::command]
pub async fn paper_search(query: String, year: Option<String>, limit: Option<usize>) -> Result<Papers, String> {
    let q = one_line(&query.replace('-', " "));
    if q.is_empty() {
        return Err("Say what kind of papers to look for.".into());
    }
    let limit = limit.unwrap_or(6).clamp(1, 20);
    let year = year.as_deref();
    let s2 = match semantic_scholar(&q, year, limit).await {
        Ok(p) if !p.is_empty() => return Ok(Papers { source: "Semantic Scholar".into(), papers: p }),
        other => other,
    };
    match arxiv(&q, year, limit).await {
        Ok(p) if !p.is_empty() => Ok(Papers { source: "arXiv".into(), papers: p }),
        Ok(_) => Err(format!("No papers found for \"{q}\". Try fewer or more general words.")),
        Err(e) => Err(match s2 {
            Err(s) => format!("{s} {e} Try again in a minute."),
            Ok(_) => e,
        }),
    }
}

// ---------- YouTube ----------

fn video_id_ok(id: &str) -> bool {
    id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A youtube.com/watch, youtu.be or youtube.com/shorts link as https://www.youtube.com/watch?v=ID.
fn youtube_url(url: &str) -> Option<String> {
    let s = url.trim();
    let s = if s.contains("://") { s.to_string() } else { format!("https://{s}") };
    let u = reqwest::Url::parse(&s).ok()?;
    let host = u.host_str()?.trim_start_matches("www.").trim_start_matches("m.");
    let id = match host {
        "youtu.be" => u.path_segments()?.next()?.to_string(),
        "youtube.com" | "music.youtube.com" => {
            let mut seg = u.path_segments()?;
            match seg.next()? {
                "watch" => u.query_pairs().find(|(k, _)| k == "v")?.1.to_string(),
                "shorts" | "live" => seg.next()?.to_string(),
                _ => return None,
            }
        }
        _ => return None,
    };
    video_id_ok(&id).then(|| format!("https://www.youtube.com/watch?v={id}"))
}

#[derive(Serialize)]
pub struct VideoAnswer {
    url: String,
    answer: String,
}

/// Ask Gemini a question about a public YouTube video (it watches and listens to it).
#[tauri::command]
pub async fn youtube_ask(app: AppHandle, url: String, question: Option<String>) -> Result<VideoAnswer, String> {
    let link = youtube_url(&url).ok_or("That isn't a YouTube video link. Use a youtube.com/watch, youtu.be or youtube.com/shorts link.")?;
    let key = settings::load(&app).gemini_api_key;
    if key.is_empty() {
        return Err("Add your Gemini API key in Settings to ask about videos.".into());
    }
    let q = question.map(|q| q.trim().to_string()).filter(|q| !q.is_empty()).unwrap_or_else(|| "Summarise this video with the key points and [MM:SS] timestamps.".into());
    let body = json!({ "contents": [{ "role": "user", "parts": [{ "fileData": { "fileUri": link } }, { "text": q }] }] });
    let http = client(120)?;
    for model in ["gemini-3.8-flash", "gemini-2.5-flash"] {
        let res = http
            .post(format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"))
            .header("x-goog-api-key", &key)
            .json(&body)
            .send()
            .await
            .map_err(|e| if e.is_timeout() { "The video took too long to process. Try a shorter video or a narrower question.".to_string() } else { format!("Couldn't reach Google: {e}") })?;
        let status = res.status();
        let v: Value = res.json().await.unwrap_or(Value::Null);
        if status == reqwest::StatusCode::NOT_FOUND {
            continue; // this model isn't available on the key; try the next
        }
        if !status.is_success() {
            let msg = v["error"]["message"].as_str().unwrap_or("");
            let lower = msg.to_lowercase();
            if lower.contains("private") || lower.contains("unlisted") || lower.contains("permission") || lower.contains("not accessible") || lower.contains("cannot be accessed") || lower.contains("unavailable") {
                return Err("That video is private, unlisted or unavailable, so Gemini can't watch it. Only public videos work.".into());
            }
            return Err(format!("Gemini couldn't watch the video ({status}): {msg}"));
        }
        let answer = v["candidates"][0]["content"]["parts"].as_array().map(|p| p.iter().filter_map(|x| x["text"].as_str()).collect::<Vec<_>>().join("")).unwrap_or_default().trim().to_string();
        if answer.is_empty() {
            return Err("Gemini watched the video but gave no answer. It may be blocked or too long.".into());
        }
        return Ok(VideoAnswer { url: link, answer });
    }
    Err("No Gemini model that can watch videos is available on this key.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tickers() -> Vec<Company> {
        parse_tickers(&json!({
            "0": { "cik_str": 1045810, "ticker": "NVDA", "title": "NVIDIA CORP" },
            "1": { "cik_str": 320193, "ticker": "AAPL", "title": "Apple Inc." },
            "2": { "cik_str": 1067983, "ticker": "BRK-B", "title": "BERKSHIRE HATHAWAY INC" },
            "3": { "cik_str": 1318605, "ticker": "TSLA", "title": "Tesla, Inc." },
            "10": { "cik_str": 999, "ticker": "APLE", "title": "Apple Hospitality REIT, Inc." }
        }))
    }

    #[test]
    fn tickers_resolve_by_ticker_cik_and_name() {
        let list = sample_tickers();
        assert_eq!(list.len(), 5);
        assert_eq!(list[0].cik, "0001045810");
        assert_eq!(find_company(&list, "nvda").unwrap().name, "NVIDIA CORP");
        assert_eq!(find_company(&list, "$AAPL").unwrap().ticker, "AAPL");
        assert_eq!(find_company(&list, "BRK.B").unwrap().ticker, "BRK-B");
        assert_eq!(find_company(&list, "320193").unwrap().ticker, "AAPL");
        assert_eq!(find_company(&list, "Apple").unwrap().ticker, "AAPL"); // not Apple Hospitality
        assert_eq!(find_company(&list, "tesla inc").unwrap().ticker, "TSLA");
        assert_eq!(find_company(&list, "berkshire").unwrap().ticker, "BRK-B");
        assert!(find_company(&list, "Nonexistent Widgets").is_none());
        assert!(find_company(&list, "  ").is_none());
    }

    #[test]
    fn submissions_columns_become_filings_with_urls() {
        let v = json!({ "filings": { "recent": {
            "accessionNumber": ["0000320193-25-000079", "0000320193-25-000070", "0000320193-24-000123"],
            "filingDate": ["2025-10-31", "2025-08-01", "2024-11-01"],
            "form": ["10-K", "10-Q", "10-K"],
            "primaryDocument": ["aapl-20250927.htm", "aapl-20250628.htm", "aapl-20240928.htm"],
            "primaryDocDescription": ["10-K", "", "Annual report"]
        }}});
        let all = parse_recent(&v, "0000320193", &[], 10);
        assert_eq!(all.len(), 3);
        assert_eq!(all[1].description, "10-Q");
        let k = parse_recent(&v, "0000320193", &["10-k".into()], 1);
        assert_eq!(k, vec![Filing { form: "10-K".into(), date: "2025-10-31".into(), description: "10-K".into(), url: "https://www.sec.gov/Archives/edgar/data/320193/000032019325000079/aapl-20250927.htm".into() }]);
        assert!(parse_recent(&json!({}), "1", &[], 5).is_empty());
    }

    #[test]
    fn full_text_hits_build_urls_and_skip_odd_rows() {
        let v = json!({ "hits": { "hits": [
            { "_id": "0000320193-25-000079:aapl-20250927.htm", "_source": { "display_names": ["Apple Inc.  (AAPL)  (CIK 0000320193)"], "form": "10-K", "file_date": "2025-10-31", "ciks": ["0000320193"], "adsh": "0000320193-25-000079" } },
            { "_id": "0000320193-25-000079:aapl-20250927.htm", "_source": { "ciks": ["0000320193"], "adsh": "0000320193-25-000079" } },
            { "_id": "broken", "_source": {} },
            { "_id": "0001045810-25-000023:nvda-20250126.htm", "_source": { "display_names": ["NVIDIA CORP"], "root_forms": ["10-K"], "file_date": "2025-02-26", "ciks": ["0001045810"] } }
        ]}});
        let h = parse_hits(&v, 10);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].url, "https://www.sec.gov/Archives/edgar/data/320193/000032019325000079/aapl-20250927.htm");
        assert_eq!(h[0].company, "Apple Inc. (AAPL) (CIK 0000320193)");
        assert_eq!(h[1].form, "10-K");
        assert_eq!(h[1].url, "https://www.sec.gov/Archives/edgar/data/1045810/000104581025000023/nvda-20250126.htm");
        assert!(parse_hits(&json!({ "hits": "nope" }), 10).is_empty());
    }

    #[test]
    fn only_sec_archive_urls_are_read() {
        assert!(archive_url("https://www.sec.gov/Archives/edgar/data/320193/x/a.htm").is_ok());
        assert!(archive_url("http://www.sec.gov/Archives/edgar/a.htm").is_err());
        assert!(archive_url("https://www.sec.gov/cgi-bin/browse-edgar").is_err());
        assert!(archive_url("https://evil.example/Archives/a.htm").is_err());
        assert!(archive_url("https://www.sec.gov.evil.example/Archives/a.htm").is_err());
        assert!(archive_url("https://www.sec.gov/Archives/../files/x").is_err());
    }

    #[test]
    fn html_becomes_readable_text() {
        let html = r#"<html><head><title>x</title><style>p{color:red}</style></head><body>
            <ix:header><ix:hidden>dei:Secret 123</ix:hidden></ix:header>
            <script>alert('no')</script><!-- a > comment -->
            <p>Risk&#160;Factors</p><div>Supply   chain &amp; <b>logistics</b> &#8212; <i>Apple&rsquo;s</i></div>
            <table><tr><td>Revenue</td><td>$391,035</td></tr></table></body></html>"#;
        let t = html_to_text(html);
        assert!(!t.contains("color") && !t.contains("alert") && !t.contains("Secret") && !t.contains("comment"));
        assert!(t.contains("Risk Factors"));
        assert!(t.contains("Supply chain & logistics — Apple's"));
        assert!(t.contains("Revenue $391,035"));
        assert!(!t.contains("\n\n\n"));
        assert_eq!(decode_entities("a &bogus; & b &#x41;"), "a &bogus; & b A");
    }

    #[test]
    fn passages_find_the_best_windows() {
        let filler = "lorem ipsum dolor sit amet ".repeat(200);
        let text = format!("{filler}Our supply chain depends on suppliers in Asia. {filler}The supply of chips is short. {filler}Chain stores. {filler}");
        let p = passages(&text, "supply chain risk", 2, 300);
        assert_eq!(p.len(), 2);
        assert!(p[0].contains("supply chain depends"));
        assert!(p[0].starts_with('…') && p[0].ends_with('…'));
        let one = passages(&text, "\"supply chain\"", 1, 300);
        assert!(one[0].contains("supply chain depends"));
        assert!(passages(&text, "zebra", 8, 300).is_empty());
        // Multi-byte characters near a window edge don't break slicing.
        let uni = format!("{}é supply é{}", "é".repeat(400), "é".repeat(400));
        assert_eq!(passages(&uni, "supply", 3, 101).len(), 1);
    }

    #[test]
    fn semantic_scholar_results_are_uniform() {
        let v = json!({ "data": [
            { "title": "Diarization  Now", "year": 2023, "citationCount": 42, "url": "https://www.semanticscholar.org/paper/abc",
              "authors": [{"name":"A"},{"name":"B"},{"name":"C"},{"name":"D"}], "tldr": { "text": "It works." },
              "abstract": "Long abstract", "openAccessPdf": { "url": "https://x/p.pdf" }, "externalIds": {} },
            { "title": "No PDF", "year": null, "authors": [{"name":"Solo"}], "abstract": "x".repeat(400), "externalIds": { "ArXiv": "2101.00001" } },
            { "year": 2020 }
        ]});
        let p = parse_s2(&v);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].title, "Diarization Now");
        assert_eq!(p[0].authors, "A, B, C et al.");
        assert_eq!(p[0].summary, "It works.");
        assert_eq!(p[0].pdf.as_deref(), Some("https://x/p.pdf"));
        assert_eq!(p[1].authors, "Solo");
        assert_eq!(p[1].summary.chars().count(), 301);
        assert_eq!(p[1].pdf.as_deref(), Some("https://arxiv.org/pdf/2101.00001"));
        assert_eq!(p[1].citations, None);
    }

    #[test]
    fn arxiv_atom_is_parsed() {
        let xml = r#"<?xml version="1.0"?><feed xmlns="http://www.w3.org/2005/Atom"><title>ArXiv Query</title>
          <entry><id>http://arxiv.org/abs/2401.00001v1</id><published>2024-01-02T00:00:00Z</published>
            <title>End-to-End Speaker
              Diarization &amp; More</title><summary>  We propose
              a model. </summary>
            <author><name>Ann Lee</name></author><author><name>Bo Chen</name><arxiv:affiliation>X</arxiv:affiliation></author>
            <link href="http://arxiv.org/abs/2401.00001v1" rel="alternate" type="text/html"/>
            <link title="pdf" href="http://arxiv.org/pdf/2401.00001v1" rel="related" type="application/pdf"/></entry>
          <entry><id>http://arxiv.org/abs/2</id><title>Bare</title></entry></feed>"#;
        let p = parse_arxiv(xml);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].title, "End-to-End Speaker Diarization & More");
        assert_eq!(p[0].year, Some(2024));
        assert_eq!(p[0].authors, "Ann Lee, Bo Chen");
        assert_eq!(p[0].summary, "We propose a model.");
        assert_eq!(p[0].pdf.as_deref(), Some("http://arxiv.org/pdf/2401.00001v1"));
        assert_eq!(p[0].url, "http://arxiv.org/abs/2401.00001v1");
        assert_eq!(p[1].pdf, None);
        assert_eq!(year_span("2020-2024"), Some((2020, 2024)));
        assert_eq!(year_span("2024"), Some((2024, 2024)));
        assert_eq!(year_span(""), None);
    }

    #[test]
    fn youtube_links_are_normalised() {
        let want = Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string());
        assert_eq!(youtube_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42s"), want);
        assert_eq!(youtube_url("youtu.be/dQw4w9WgXcQ?si=abc"), want);
        assert_eq!(youtube_url("https://m.youtube.com/watch?feature=share&v=dQw4w9WgXcQ"), want);
        assert_eq!(youtube_url("https://youtube.com/shorts/dQw4w9WgXcQ"), want);
        assert_eq!(youtube_url("https://www.youtube.com/playlist?list=PL123"), None);
        assert_eq!(youtube_url("https://vimeo.com/123"), None);
        assert_eq!(youtube_url("https://youtube.com.evil.example/watch?v=dQw4w9WgXcQ"), None);
        assert_eq!(youtube_url("https://youtu.be/short"), None);
    }

    /// Live checks against the real services. Run with:
    /// cargo test --lib sources::tests::live -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live() {
        let f = sec_filings("AAPL".into(), Some(vec!["10-K".into()]), Some(3)).await.unwrap();
        println!("sec_filings AAPL 10-K: {} -> {}", f.company.name, serde_json::to_string_pretty(&f.filings).unwrap());
        let h = sec_search("\"supply chain\"".into(), Some(vec!["10-K".into()]), Some("2025-01-01".into()), Some("2025-12-31".into())).await.unwrap();
        println!("sec_search: {}", serde_json::to_string_pretty(&h).unwrap());
        let d = sec_read(f.filings[0].url.clone(), Some("supply chain risk".into())).await.unwrap();
        println!("sec_read with query: {} passages, {} chars; first: {}", d.passages.unwrap_or(0), d.text.len(), chars(&d.text, 300));
        let full = sec_read(f.filings[0].url.clone(), None).await.unwrap();
        println!("sec_read full: truncated={} first: {}", full.truncated, chars(&full.text, 200));
        let p = paper_search("speech diarization".into(), None, Some(5)).await.unwrap();
        println!("paper_search ({}): {}", p.source, serde_json::to_string_pretty(&p.papers).unwrap());
        let a = arxiv("speech diarization", None, 3).await.unwrap();
        println!("arxiv fallback: {}", serde_json::to_string_pretty(&a).unwrap());
    }
}
