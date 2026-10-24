//! `ww pins tui`: every point of a device serving `ww_pin_diag::PinDiag` live, laid out per connector from a board
//! preset (TOOL-21). The layout, cursor and what is done to the points are `ww_pin_probe::live`; this draws them
//! and turns keys into commands.

use crate::untrusted::escape;
use crate::util::StopSignal;
use anyhow::{Result, anyhow};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;
use ww_pin_probe::board::{Board, Role, is_anonymous};
use ww_pin_probe::live::{Command, Cursor, Dir, Layout, Live, Slot, describe};
use ww_pin_probe::{Level, Pins, Point, PointCaps, PointState, Pull};

/// The keys, as the bottom line shows them.
const KEYS: &str = "←↑↓→/hjkl move  Tab connector  1/0 drive high/low  t toggle  u/d/n pull-up/down/none  \
                    w weak  r read  x release all  p pause  q quit";

/// Longest net or point name shown in a cell; the detail line has the rest.
const NAME_MAX: usize = 12;

/// The view: what [Live] knows, where it is shown, the cursor.
pub(crate) struct App {
    live: Live,
    layout: Layout,
    board: Option<Board>,
    cursor: Option<Cursor>,
    /// `PinDiag at diag.pins of <device>`.
    title: String,
    paused: bool,
    interval: Duration,
    /// First line of the panels shown, so the cursor stays in view.
    scroll: u16,
}

impl App {
    pub(crate) fn new(
        points: Vec<Point>,
        board: Option<Board>,
        title: String,
        interval: Duration,
    ) -> Self {
        let layout = Layout::new(board.as_ref(), &points);
        let cursor = layout.first();
        App {
            live: Live::new(points),
            layout,
            board,
            cursor,
            title,
            paused: false,
            interval,
            scroll: 0,
        }
    }

    fn point(&self) -> Option<u16> {
        self.cursor
            .and_then(|c| self.layout.slot(c))
            .and_then(|s| s.point)
    }

    /// Reads all points, unless paused.
    pub(crate) async fn refresh(&mut self, pins: &mut impl Pins) -> Result<()> {
        if self.paused {
            return Ok(());
        }
        self.live.read_all(pins).await.map_err(|e| anyhow!(e))
    }

    /// Does what `key` asks; `Ok(false)` to quit. `Err` when the link failed.
    pub(crate) async fn key(&mut self, pins: &mut impl Pins, key: KeyEvent) -> Result<bool> {
        if key.kind == KeyEventKind::Release {
            return Ok(true);
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let cmd = match key.code {
            KeyCode::Char('c') if ctrl => return Ok(false),
            KeyCode::Char('q') | KeyCode::Esc => return Ok(false),
            KeyCode::Up | KeyCode::Char('k') => return Ok(self.step(Dir::Up)),
            KeyCode::Down | KeyCode::Char('j') => return Ok(self.step(Dir::Down)),
            KeyCode::Left | KeyCode::Char('h') => return Ok(self.step(Dir::Left)),
            KeyCode::Right | KeyCode::Char('l') => return Ok(self.step(Dir::Right)),
            KeyCode::Tab | KeyCode::BackTab => {
                if let Some(c) = self.cursor {
                    self.cursor = Some(self.layout.next_panel(c, key.code == KeyCode::BackTab));
                }
                return Ok(true);
            }
            KeyCode::Char('p') => {
                self.paused = !self.paused;
                let what = if self.paused { "paused" } else { "running" };
                self.live.message = Some((false, format!("reading {what}")));
                return Ok(true);
            }
            KeyCode::Char('1') => Command::Drive(Level::High),
            KeyCode::Char('0') => Command::Drive(Level::Low),
            KeyCode::Char('t') | KeyCode::Char(' ') => Command::Toggle,
            KeyCode::Char('u') => Command::Input(Pull::Up),
            KeyCode::Char('d') => Command::Input(Pull::Down),
            KeyCode::Char('n') => Command::Input(Pull::None),
            KeyCode::Char('w') => Command::Weak,
            KeyCode::Char('r') => Command::ReadAll,
            KeyCode::Char('x') | KeyCode::Char('R') => Command::Release,
            _ => return Ok(true),
        };
        let point = self.point();
        self.live
            .command(pins, point, cmd)
            .await
            .map_err(|e| anyhow!(e))?;
        Ok(true)
    }

    fn step(&mut self, dir: Dir) -> bool {
        if let Some(c) = self.cursor {
            self.cursor = Some(self.layout.step(c, dir));
        }
        true
    }

    pub(crate) fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        self.draw(area, frame.buffer_mut());
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer) {
        if area.height < 6 || area.width < 20 {
            buf.set_string(area.x, area.y, "terminal too small", Style::new());
            return;
        }
        let dim = Style::new().fg(Color::DarkGray);
        let held = self.live.held();
        let mut top = format!("{} · {} points · ", self.title, self.live.points.len());
        top += &if self.paused {
            "reading paused".to_string()
        } else {
            format!("read every {} ms", self.interval.as_millis())
        };
        if self.live.weak {
            top += " · weak drive";
        }
        put(
            buf,
            area,
            0,
            0,
            &top,
            Style::new().add_modifier(Modifier::BOLD),
        );
        if held > 0 {
            let x = top.chars().count() as u16;
            let s = format!(" · {held} set, x releases");
            put(buf, area, x, 0, &s, Style::new().fg(Color::Yellow));
        }

        // panels between the title and the three lines at the bottom
        let body = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height - 4,
        };
        let placed = self.place(body.width);
        if let Some(c) = self.cursor
            && let Some((_, y, _, _)) = placed.iter().find(|p| p.0 == c.panel).map(|p| p.1)
        {
            // the cursor's line: the panel's border plus its row
            let line = y + 1 + c.row as u16;
            if line < self.scroll {
                self.scroll = line.saturating_sub(1);
            } else if line + 2 > self.scroll + body.height {
                self.scroll = line + 2 - body.height;
            }
        }
        for (panel, (x, y, w, h)) in &placed {
            self.draw_panel(buf, body, *panel, *x, *y, *w, *h);
        }

        let bottom = area.y + area.height - 3;
        let line = Rect {
            x: area.x,
            y: bottom,
            width: area.width,
            height: 3,
        };
        put(buf, line, 0, 0, &self.detail(), Style::new());
        if let Some((refused, msg)) = &self.live.message {
            let style = if *refused {
                Style::new().fg(Color::Red)
            } else {
                Style::new().fg(Color::Green)
            };
            put(buf, line, 0, 1, &escape(msg), style);
        }
        put(buf, line, 0, 2, KEYS, dim);
    }

    /// Where each panel goes on a virtual page `width` wide: the preset's columns left to right, a column that
    /// doesn't fit starts a new band below. `(panel, (x, y, width, height))`.
    fn place(&self, width: u16) -> Vec<(usize, (u16, u16, u16, u16))> {
        let mut out = vec![];
        let (mut x, mut band_y, mut band_h) = (0u16, 0u16, 0u16);
        for col in &self.layout.columns {
            let w = col.iter().map(|&p| self.panel_width(p)).max().unwrap_or(0);
            let h: u16 = col
                .iter()
                .map(|&p| self.layout.panels[p].rows.len() as u16 + 2)
                .sum();
            if x > 0 && x + w > width {
                x = 0;
                band_y += band_h;
                band_h = 0;
            }
            let mut y = band_y;
            for &p in col {
                let ph = self.layout.panels[p].rows.len() as u16 + 2;
                out.push((p, (x, y, w, ph)));
                y += ph;
            }
            x += w + 1;
            band_h = band_h.max(h);
        }
        out
    }

    /// Widths of a panel's pad numbers and names, and of a whole cell: `name number marks`.
    fn cell(&self, panel: usize) -> (usize, usize, usize) {
        let p = &self.layout.panels[panel];
        let slots = p.rows.iter().flatten().flatten();
        let num = slots
            .clone()
            .map(|s| s.pad.as_ref().map_or(0, |p| p.number.chars().count()))
            .max()
            .unwrap_or(0);
        let name = slots
            .map(|s| self.name(s).chars().count())
            .max()
            .unwrap_or(0)
            .clamp(1, NAME_MAX);
        let cell = name + 1 + if num > 0 { num + 1 } else { 0 } + 2;
        (num, name, cell)
    }

    fn panel_width(&self, panel: usize) -> u16 {
        let p = &self.layout.panels[panel];
        let cols = p.rows.iter().map(Vec::len).max().unwrap_or(1);
        let inner = cols * self.cell(panel).2 + (cols - 1) * 2;
        // "─ title ─" between the corners
        let title = p.title.chars().count() + 4;
        inner.max(title) as u16 + 2
    }

    /// A slot's name: the pad's net (its alias when the board file made the net's name up: `VDD_TARGET` for
    /// `NetCN11_1`), or the point's name.
    fn name(&self, s: &Slot) -> String {
        if let Some(pad) = &s.pad {
            let net = pad.name().unwrap_or("");
            let alias = self.board.as_ref().and_then(|b| b.alias(pad));
            let name = match alias {
                Some(alias) if is_anonymous(net) => alias,
                _ => net,
            };
            return escape(name).into_owned();
        }
        s.point
            .map(|id| escape(&self.live.points[id as usize].name).into_owned())
            .unwrap_or_default()
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_panel(
        &self,
        buf: &mut Buffer,
        body: Rect,
        panel: usize,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
    ) {
        let p = &self.layout.panels[panel];
        let border = Style::new().fg(Color::DarkGray);
        let at = |line: u16| line.checked_sub(self.scroll);
        let horizontal = "─".repeat(w.saturating_sub(2) as usize);
        if let Some(l) = at(y) {
            put(buf, body, x, l, &format!("┌{horizontal}┐"), border);
            put(
                buf,
                body,
                x + 2,
                l,
                &format!(" {} ", escape(&p.title)),
                Style::new().fg(Color::Cyan),
            );
        }
        if let Some(l) = at(y + h - 1) {
            put(buf, body, x, l, &format!("└{horizontal}┘"), border);
        }
        let widths = self.cell(panel);
        let cw = widths.2;
        let cols = p.rows.iter().map(Vec::len).max().unwrap_or(1);
        for (r, row) in p.rows.iter().enumerate() {
            let Some(l) = at(y + 1 + r as u16) else {
                continue;
            };
            put(buf, body, x, l, "│", border);
            put(buf, body, x + w - 1, l, "│", border);
            for (c, slot) in row.iter().enumerate() {
                let Some(slot) = slot else { continue };
                // a two-row header: the left row mirrored, so the marks sit next to each other as the pads do
                let mirrored = cols == 2 && c == 0;
                let cx = x + 1 + (c * (cw + 2)) as u16;
                let selected = self.cursor
                    == Some(Cursor {
                        panel,
                        row: r,
                        col: c,
                    });
                self.draw_slot(buf, body, cx, l, widths, slot, mirrored, selected);
            }
        }
    }

    /// One slot: `marks number name`, or mirrored `name number marks`, the level mark next to the pad.
    #[allow(clippy::too_many_arguments)]
    fn draw_slot(
        &self,
        buf: &mut Buffer,
        body: Rect,
        x: u16,
        y: u16,
        (num_w, name_w, cell_w): (usize, usize, usize),
        slot: &Slot,
        mirrored: bool,
        selected: bool,
    ) {
        let (set, level) = self.marks(slot);
        let mut name: String = self.name(slot).chars().take(NAME_MAX).collect();
        if name.is_empty() {
            name = "·".into();
        }
        let number = slot.pad.as_ref().map_or("", |p| p.number.as_str());
        let number = match num_w {
            0 => String::new(),
            w => format!("{number:>w$} "),
        };
        let base = if selected {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new()
        };
        if selected {
            put(buf, body, x, y, &" ".repeat(cell_w), base);
        }
        let num_style = Style::new().fg(Color::DarkGray).patch(base);
        let name_style = self.name_style(slot).patch(base);
        let width = |s: &str| s.chars().count() as u16;
        if mirrored {
            let name = format!("{name:>name_w$} ");
            put(buf, body, x, y, &name, name_style);
            let x = x + width(&name);
            put(buf, body, x, y, &number, num_style);
            let x = x + width(&number);
            put(buf, body, x, y, &set.0, set.1.patch(base));
            put(buf, body, x + 1, y, &level.0, level.1.patch(base));
        } else {
            put(buf, body, x, y, &level.0, level.1.patch(base));
            put(buf, body, x + 1, y, &set.0, set.1.patch(base));
            put(buf, body, x + 3, y, &number, num_style);
            put(buf, body, x + 3 + width(&number), y, &name, name_style);
        }
    }

    /// What was set (`▲` driving high, `▼` low, `↑` `↓` pulls, `→` input) and the level read (`●` high, `○` low,
    /// `×` refused), each with its style.
    fn marks(&self, slot: &Slot) -> ((String, Style), (String, Style)) {
        let Some(id) = slot.point else {
            return ((" ".into(), Style::new()), (" ".into(), Style::new()));
        };
        let s = &self.live.state[id as usize];
        let yellow = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
        let set = match s.set {
            Some(PointState::Output {
                level: Level::High, ..
            }) => "▲",
            Some(PointState::Output {
                level: Level::Low, ..
            }) => "▼",
            Some(PointState::Input { pull: Pull::Up }) => "↑",
            Some(PointState::Input { pull: Pull::Down }) => "↓",
            Some(_) => "→",
            None => " ",
        };
        let level = match (s.level, &s.read_refused) {
            (Some(Level::High), _) => ("●", Style::new().fg(Color::Green)),
            (Some(Level::Low), _) => ("○", Style::new().fg(Color::Blue)),
            (None, Some(_)) => ("×", Style::new().fg(Color::Red)),
            (None, None) => ("·", Style::new().fg(Color::DarkGray)),
        };
        ((set.into(), yellow), (level.0.into(), level.1))
    }

    /// Pads without a point dim; a pin the preset marks taken or fixed red, shared yellow.
    fn name_style(&self, slot: &Slot) -> Style {
        let role = slot
            .pad
            .as_ref()
            .and_then(|pad| self.board.as_ref()?.pin_info(pad))
            .map(|i| i.role);
        let style = match role {
            Some(Role::Taken | Role::Function) => Style::new().fg(Color::Red),
            Some(Role::Shared) => Style::new().fg(Color::Yellow),
            _ => Style::new(),
        };
        if slot.point.is_none() {
            style.add_modifier(Modifier::DIM)
        } else {
            style
        }
    }

    /// The line about the slot under the cursor.
    fn detail(&self) -> String {
        let Some(slot) = self.cursor.and_then(|c| self.layout.slot(c)) else {
            return "the device has no points".into();
        };
        let mut parts = vec![];
        if let (Some(pad), Some(c)) = (&slot.pad, self.cursor) {
            let r#ref = self
                .board
                .as_ref()
                .and_then(|b| {
                    let title = &self.layout.panels[c.panel].title;
                    b.connectors.iter().find(|k| &k.label == title)
                })
                .map_or("", |k| k.r#ref.as_str());
            let mut s = format!(
                "{}-{} {}",
                r#ref,
                pad.number,
                pad.name().unwrap_or("not connected")
            );
            if let Some(alias) = self.board.as_ref().and_then(|b| b.alias(pad)) {
                s += &format!(" ({alias})");
            }
            parts.push(s);
            if let Some(info) = self.board.as_ref().and_then(|b| b.pin_info(pad)) {
                let mut s = info.role.as_str().to_string();
                if !info.short.is_empty() {
                    s += &format!(" {}", info.short);
                }
                parts.push(s);
            }
        }
        match slot.point {
            Some(id) => {
                let p = &self.live.points[id as usize];
                parts.push(format!("P{id} {} [{}]", p.name, p.group));
                let s = &self.live.state[id as usize];
                parts.push(match (s.level, &s.read_refused) {
                    (Some(Level::High), _) => "reads high".into(),
                    (Some(Level::Low), _) => "reads low".into(),
                    (None, Some(why)) => format!("read: {why}"),
                    (None, None) => "not read yet".into(),
                });
                if let Some(state) = s.set {
                    parts.push(format!("set: {}", describe(state)));
                }
                parts.push(caps(p.caps));
            }
            None if slot.pad.as_ref().is_some_and(|p| p.net.is_some()) => {
                parts.push("no point of the device".into())
            }
            None => {}
        }
        escape(&parts.join(" · ")).into_owned()
    }
}

fn caps(c: PointCaps) -> String {
    let all = [
        (c.read, "read"),
        (c.pull_up, "pull-up"),
        (c.pull_down, "pull-down"),
        (c.drive_low, "low"),
        (c.drive_high, "high"),
        (c.weak_drive, "weak"),
    ];
    let can: Vec<&str> = all.iter().filter(|(y, _)| *y).map(|(_, n)| *n).collect();
    format!("can: {}", can.join(" "))
}

/// Writes `text` at (`x`, `y`) of `area`, cut at its edges.
fn put(buf: &mut Buffer, area: Rect, x: u16, y: u16, text: &str, style: Style) {
    if y >= area.height || x >= area.width {
        return;
    }
    let room = (area.width - x) as usize;
    buf.set_stringn(area.x + x, area.y + y, text, room, style);
}

/// Runs the view on the terminal until `q`, Ctrl-C or a signal; every point is released at the end, also after an
/// error.
pub(crate) async fn run(app: &mut App, pins: &mut impl Pins, mut stop: StopSignal) -> Result<()> {
    let mut terminal = ratatui::init();
    let done = Arc::new(AtomicBool::new(false));
    let (tx, mut keys) = mpsc::unbounded_channel();
    let reader = {
        let done = done.clone();
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                match event::poll(Duration::from_millis(50)) {
                    Ok(true) => match event::read() {
                        Ok(e) => {
                            if tx.send(e).is_err() {
                                return;
                            }
                        }
                        Err(_) => return,
                    },
                    Ok(false) => {}
                    Err(_) => return,
                }
            }
        })
    };
    let result: Result<()> = async {
        app.refresh(pins).await?;
        let mut tick = tokio::time::interval(app.interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            terminal.draw(|f| app.render(f))?;
            tokio::select! {
                e = keys.recv() => match e {
                    Some(Event::Key(key)) => {
                        if !app.key(pins, key).await? {
                            return Ok(());
                        }
                    }
                    Some(_) => {}
                    None => return Ok(()),
                },
                _ = tick.tick() => app.refresh(pins).await?,
                _ = stop.wait() => return Ok(()),
            }
        }
    }
    .await;
    done.store(true, Ordering::Relaxed);
    _ = reader.join();
    ratatui::restore();
    let released = app.live.release(pins).await;
    match (result, released) {
        // both said: points left set matter as much as why the view stopped
        (Err(e), Err(r)) => Err(e.context(format!("and releasing the points failed too: {r}"))),
        (Err(e), Ok(())) => Err(e),
        (Ok(()), Err(e)) => Err(anyhow!("releasing the points: {e}")),
        (Ok(()), Ok(())) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::Path;
    use ww_pin_probe::mock::{ALL, MockBoard, Net};

    fn key(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    fn screen(app: &mut App, w: u16, h: u16) -> String {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| app.render(f)).unwrap();
        let buf = t.backend().buffer();
        (0..h)
            .map(|y| {
                let line: String = (0..w).map(|x| buf[(x, y)].symbol()).collect();
                line.trim_end().to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// A mock of the Nucleo's firmware: some of its free pins, the button held by its function, one point that is
    /// on no connector.
    fn nucleo_mock() -> MockBoard {
        let mut b = MockBoard::default();
        b.point("PC10", "mcu", ALL, Net::Open);
        let pc11 = b.point("PC11", "mcu", ALL, Net::Open);
        b.shorted_to("PC12", "mcu", ALL, pc11);
        b.point("PC13", "mcu", ALL, Net::PullUp);
        b.points[3].held_by = Some("button".into());
        b.point("PA5", "mcu", ALL, Net::Open);
        b.point("EX0", "chain", ALL, Net::Open);
        b
    }

    fn nucleo() -> Board {
        Board::load(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../ww_pin_probe/tests/data/nucleo_g0b1re.toml"),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn nucleo_preset_on_a_mock_board() {
        let mut b = nucleo_mock();
        let mut app = App::new(
            b.host_points(),
            Some(nucleo()),
            "PinDiag at (root)".into(),
            Duration::from_millis(250),
        );
        app.refresh(&mut b).await.unwrap();
        let s = screen(&mut app, 200, 30);
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines[0], "PinDiag at (root) · 6 points · read every 250 ms");
        assert!(lines[1].starts_with("┌─ CN7 ST morpho ─"), "{s}");
        // CN7-1 PC10 and CN7-2 PC11, read low, the marks next to each other as the pads
        assert!(lines[2].contains("PC10  1  ○  ○   2 PC11"), "{s}");
        // made-up net names show their alias: NetCN11_1 is VDD_TARGET
        assert!(lines[2].contains("│   1 VDD_TARGET │"), "{s}");
        assert!(s.contains("not on a connector"), "{s}");
        assert!(
            lines[27].starts_with("CN7-1 PC10 · P0 PC10 [mcu] · reads low"),
            "{s}"
        );

        // drive PC11 high: PC12 on its net follows
        app.key(&mut b, key(KeyCode::Right)).await.unwrap();
        app.key(&mut b, key(KeyCode::Char('1'))).await.unwrap();
        app.refresh(&mut b).await.unwrap();
        let s = screen(&mut app, 200, 30);
        let lines: Vec<&str> = s.lines().collect();
        assert!(lines[2].contains("PC10  1  ○  ●▲  2 PC11"), "{s}");
        assert!(lines[3].contains("PC12  3  ●"), "{s}");
        assert!(lines[0].ends_with("· 1 set, x releases"), "{s}");
        assert_eq!(lines[28], "PC11: drive high");

        // the button: refused, said in red at the bottom
        let at = app.layout.find(3).unwrap();
        app.cursor = Some(at);
        app.key(&mut b, key(KeyCode::Char('0'))).await.unwrap();
        let s = screen(&mut app, 200, 30);
        assert!(s.contains("PC13 drive low: in use by button"), "{s}");
        assert!(
            s.contains("CN7-23 PC13 · shared B1 · P3 PC13 [mcu] · reads high"),
            "{s}"
        );

        app.key(&mut b, key(KeyCode::Char('x'))).await.unwrap();
        assert_eq!(b.releases, 1);
        assert!(!app.key(&mut b, key(KeyCode::Char('q'))).await.unwrap());
    }

    #[tokio::test]
    async fn without_a_preset_a_panel_per_group() {
        let mut b = nucleo_mock();
        let mut app = App::new(
            b.host_points(),
            None,
            "PinDiag".into(),
            Duration::from_millis(100),
        );
        app.refresh(&mut b).await.unwrap();
        let s = screen(&mut app, 80, 14);
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines[1], "┌─ mcu ─┐ ┌─ chain ─┐");
        assert_eq!(lines[2], "│○  PC10│ │○  EX0   │");
        assert_eq!(lines[5], "│●  PC13│");
        // small terminals wrap the columns into bands
        let s = screen(&mut app, 20, 20);
        assert!(s.lines().nth(8).unwrap().starts_with("┌─ chain"), "{s}");
        // a failed link ends the view with the error
        b.fail_after = Some(b.calls);
        assert!(app.refresh(&mut b).await.is_err());
    }
}
