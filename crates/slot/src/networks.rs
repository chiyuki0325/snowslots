use crate::update::signal_bars;
use crate::wifi::{Failure, Phase, Saved, Scanned, Snapshot, State};

pub const OTHER_NETWORK: &str = "其他网络";
pub const ENTER_PASSWORD: &str = "输入密码";
pub const SCAN_NETWORKS: &str = "搜索网络";
pub const WIFI_OFF_MESSAGE: &str = "打开Wi-Fi后可查看网络";
pub const SCANNING_MESSAGE: &str = "正在搜索网络";
pub const PASSWORD_TITLE: &str = "密码";
pub const NAME_TITLE: &str = "网络名称";
pub const OPEN_HINT: &str = "开放网络无需填写密码";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetRow {
    Saved(Saved),
    Nearby(Scanned),
    Other,
    Scan,
}

impl NetRow {
    pub fn name(&self) -> Option<&str> {
        match self {
            NetRow::Saved(s) => Some(&s.ssid),
            NetRow::Nearby(n) => Some(&n.ssid),
            NetRow::Other | NetRow::Scan => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub label: String,
    pub value: Option<String>,
    pub lock: bool,
    pub bars: Option<u8>,
}

impl Line {
    fn plain(label: &str, value: Option<String>) -> Line {
        Line {
            label: label.to_string(),
            value,
            lock: false,
            bars: None,
        }
    }
}

pub fn nearby(snap: &Snapshot) -> Vec<Scanned> {
    let mut out: Vec<Scanned> = Vec::new();
    for n in snap.scan.iter().flatten().filter(|n| !n.ssid.is_empty()) {
        if snap.saved.iter().any(|s| s.ssid == n.ssid) {
            continue;
        }
        match out.iter_mut().find(|o| o.ssid == n.ssid) {
            Some(o) if o.signal < n.signal => *o = n.clone(),
            Some(_) => {}
            None => out.push(n.clone()),
        }
    }
    out.sort_by(|a, b| b.signal.cmp(&a.signal).then_with(|| a.ssid.cmp(&b.ssid)));
    out
}

pub fn list_rows(snap: &Snapshot, off: bool) -> Vec<NetRow> {
    let mut rows: Vec<NetRow> = Vec::new();
    for s in &snap.saved {
        if !rows.iter().any(|r| r.name() == Some(s.ssid.as_str())) {
            rows.push(NetRow::Saved(s.clone()));
        }
    }
    if !off {
        rows.push(NetRow::Scan);
    }
    rows
}

pub fn list_message(off: bool) -> Option<&'static str> {
    off.then_some(WIFI_OFF_MESSAGE)
}

pub fn scan_rows(snap: &Snapshot) -> Vec<NetRow> {
    if snap.scan.is_none() {
        return Vec::new();
    }
    let mut rows: Vec<NetRow> = nearby(snap).into_iter().map(NetRow::Nearby).collect();
    rows.push(NetRow::Other);
    rows
}

pub fn scan_message(snap: &Snapshot) -> Option<&'static str> {
    snap.scan.is_none().then_some(SCANNING_MESSAGE)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetPick {
    Top,
    Name(String),
    Other,
    Scan,
}

impl NetPick {
    pub fn of(row: &NetRow) -> NetPick {
        match row {
            NetRow::Saved(_) | NetRow::Nearby(_) => {
                NetPick::Name(row.name().unwrap_or_default().to_string())
            }
            NetRow::Other => NetPick::Other,
            NetRow::Scan => NetPick::Scan,
        }
    }
}

pub fn selected(rows: &[NetRow], pick: &NetPick) -> usize {
    let at = match pick {
        NetPick::Top => return 0,
        NetPick::Name(n) => rows.iter().position(|r| r.name() == Some(n)),
        NetPick::Other => rows.iter().position(|r| *r == NetRow::Other),
        NetPick::Scan => rows.iter().position(|r| *r == NetRow::Scan),
    };
    at.unwrap_or(0)
}

fn in_progress(snap: &Snapshot, ssid: &str) -> Option<Phase> {
    match &snap.attempt {
        Some((s, p)) if s == ssid && !matches!(p, Phase::Connected | Phase::Failed(_)) => Some(*p),
        _ => None,
    }
}

fn ours(snap: &Snapshot, ssid: &str) -> bool {
    snap.status.ssid.as_deref() == Some(ssid)
}

pub fn connected(snap: &Snapshot, ssid: &str) -> bool {
    ours(snap, ssid) && snap.status.state == State::Completed && snap.status.ip.is_some()
}

pub fn active(snap: &Snapshot, ssid: &str) -> bool {
    in_progress(snap, ssid).is_some()
        || (ours(snap, ssid)
            && matches!(
                snap.status.state,
                State::Associating | State::Handshake | State::Completed
            ))
}

pub fn list_line(row: &NetRow, snap: &Snapshot) -> Line {
    match row {
        NetRow::Saved(s) => {
            let value = if connected(snap, &s.ssid) {
                Some("已连接")
            } else if active(snap, &s.ssid) {
                Some("正在连接")
            } else {
                None
            };
            Line::plain(&s.ssid, value.map(str::to_string))
        }
        NetRow::Nearby(n) => Line {
            label: n.ssid.clone(),
            value: None,
            lock: n.secured,
            bars: Some(signal_bars(n.signal)),
        },
        NetRow::Other => Line::plain(OTHER_NETWORK, None),
        NetRow::Scan => Line::plain(SCAN_NETWORKS, None),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetScreen {
    pub lines: Vec<Line>,
    pub selected: usize,
    pub message: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageRow {
    Status,
    Ip,
    Connect,
    Disconnect,
    Password,
    AutoJoin,
    Forget,
}

impl PageRow {
    pub fn label(self) -> &'static str {
        match self {
            PageRow::Status => "状态",
            PageRow::Ip => "IP地址",
            PageRow::Connect => "连接",
            PageRow::Disconnect => "断开",
            PageRow::Password => ENTER_PASSWORD,
            PageRow::AutoJoin => "自动连接",
            PageRow::Forget => "忘记网络",
        }
    }
}

pub fn page_rows(snap: &Snapshot, ssid: &str, off: bool) -> Vec<PageRow> {
    let mut rows = vec![PageRow::Status, PageRow::Ip];
    let saved = snap.saved.iter().any(|s| s.ssid == ssid);
    let rejected = matches!(&snap.failure, Some((s, Failure::WrongPassword)) if s == ssid);
    if !off && saved {
        rows.push(match active(snap, ssid) {
            true => PageRow::Disconnect,
            false => PageRow::Connect,
        });
    }
    if !off && rejected {
        rows.push(PageRow::Password);
    }
    if saved {
        rows.extend([PageRow::AutoJoin, PageRow::Forget]);
    }
    rows
}

pub fn status_text(snap: &Snapshot, ssid: &str, off: bool) -> String {
    if off {
        return "Wi-Fi已关闭".into();
    }
    if let Some((s, f)) = &snap.failure {
        if s == ssid {
            return f.label().into();
        }
    }
    if let Some(p) = in_progress(snap, ssid) {
        return p.label().into();
    }
    if !ours(snap, ssid) {
        return "未连接".into();
    }
    match snap.status.state {
        State::Completed if snap.status.ip.is_some() => Phase::Connected.label(),
        State::Completed => Phase::GettingIp.label(),
        State::Handshake => Phase::Authenticating.label(),
        State::Associating => Phase::Connecting.label(),
        _ => "未连接",
    }
    .into()
}

pub fn page_line(row: PageRow, snap: &Snapshot, ssid: &str, off: bool) -> Line {
    let value = match row {
        PageRow::Status => Some(status_text(snap, ssid, off)),
        PageRow::Ip => Some(
            snap.status
                .ip
                .clone()
                .filter(|_| !off && ours(snap, ssid))
                .unwrap_or_else(|| "无".into()),
        ),
        PageRow::AutoJoin => snap
            .saved
            .iter()
            .find(|s| s.ssid == ssid)
            .map(|s| if s.auto_join { "开" } else { "关" }.to_string()),
        _ => None,
    };
    Line::plain(row.label(), value)
}
