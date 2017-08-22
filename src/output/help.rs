// Std
use std::cmp;
use std::collections::BTreeMap;
use std::io::{self, Cursor, Read, Write};
use std::usize;

// Internal
use {Arg, App, AppSettings, ArgSettings};
use parsing::{DispOrder, Parser};
use output::{Error as ClapError, Result as ClapResult};
use output::fmt::{Format, Colorizer, ColorizerOption};

// Third Party
use unicode_width::UnicodeWidthStr;
#[cfg(feature = "wrap_help")]
use term_size;
#[cfg(feature = "wrap_help")]
use textwrap;
use vec_map::VecMap;

#[cfg(not(feature = "wrap_help"))]
mod term_size {
    pub fn dimensions() -> Option<(usize, usize)> { None }
}

fn str_width(s: &str) -> usize { UnicodeWidthStr::width(s) }

const TAB: &'static str = "    ";

impl<'b, 'c> DispOrder for App<'b, 'c> {
    fn disp_ord(&self) -> usize { 999 }
}

macro_rules! color {
    ($_self:ident, $w:expr, $s:expr, $c:ident) => {
        if $_self.color {
            write!($w, "{}", $_self.cizer.$c($s))
        } else {
            write!($w, "{}", $s)
        }
    };
    ($_self:ident, $w:expr, $fmt_s:expr, $v:expr, $c:ident) => {
        if $_self.color {
            write!($w, "{}", $_self.cizer.$c(format!($fmt_s, $v)))
        } else {
            write!($w, $fmt_s, $v)
        }
    };
}

/// `clap` Help Writer.
///
/// Wraps a writer stream providing different methods to generate help for `clap` objects.
pub struct HelpWriter<'a, 'b, 'c, 'd> where 'a: 'b, 'b: 'c, 'c: 'd {
    parser: &'d Parser<'a, 'b, 'c>,
    next_line_help: bool,
    hide_pv: bool,
    term_width: usize,
    color: bool,
    cizer: Colorizer,
    longest: usize,
    force_next_line: bool,
    use_long: bool,
}

// Public Initialization Functions
impl<'a, 'b, 'c, 'd> HelpWriter<'a, 'b, 'c, 'd> {
    /// Create a new `Help` instance.
    pub fn new(p: &'d Parser<'a, 'b, 'c>, use_stderr: bool) -> Self {
        debugln!("HelpWriter::new;");
        // @DESIGN @TODO-v3-beta: shouldn't use_stderr be determined by the Write object passed in 
        // later??
        let nlh = p.is_set(AppSettings::NextLineHelp);
        let hide_v = p.is_set(AppSettings::HidePossibleValuesInHelp);
        let color = p.is_set(AppSettings::ColoredHelp);
        let cizer = Colorizer::new(ColorizerOption {
            use_stderr: use_stderr,
            when: p.color(),
        });
        HelpWriter {
            parser: p,
            next_line_help: nlh,
            hide_pv: hide_v,
            term_width: match p.app.term_width {
                Some(width) => if width == 0 { usize::MAX } else { width },
                None => {
                    cmp::min(
                        term_size::dimensions().map_or(120, |(w, _)| w),
                        match p.app.max_term_width {
                            None | Some(0) => usize::MAX,
                            Some(mw) => mw,
                        },
                    )
                }
            },
            color: color,
            cizer: cizer,
            longest: 0,
            force_next_line: false,
            use_long: false,
        }
    }

    /// Reads help settings from an App
    /// and write its help to the wrapped stream.
    pub fn write_help<W: Write>(&mut self, w: &mut W) -> ClapResult<()> {
        self._write_help(w, false)
    }

    /// Reads help settings from an App
    /// and write its help to the wrapped stream.
    pub fn write_long_help<W: Write>(&mut self, w: &mut W) -> ClapResult<()> {
        self._write_help(w, true)
    }

    #[doc(hidden)]
    pub fn _write_help<W: Write>(&mut self, w: &mut W, use_long: bool) -> ClapResult<()> {
        debugln!("HelpWriter::write_app_help;");
        // @TODO-v3-alpha: Derive Display Order
        self.use_long = use_long;

        debugln!("HelpWriter::write_help;");
        if let Some(h) = self.parser.app.override_help {
            write!(w, "{}", h).map_err(ClapError::from)?;
        } else if let Some(tmpl) = self.parser.app.help_template {
            self.write_templated_help(w, tmpl)?;
        } else {
            self.write_default_help(w)?;
        }
        Ok(())
    }

    /// Writes the version to the wrapped stream
    pub fn write_version<W: Write>(&mut self, w: &mut W) -> ClapResult<()> {
        self._write_version(w, false)?;
        Ok(())
    }

    /// Writes the long version to the wrapped stream
    pub fn write_long_version<W: Write>(&mut self, w: &mut W) -> ClapResult<()> {
        self._write_version(w, true)?;
        Ok(())
    }

    #[doc(hidden)]
    fn _write_version<W: Write>(&mut self, w: &mut W, use_long: bool) -> io::Result<()> {
        let ver = if use_long {
            self.parser
                .app
                .long_version
                .unwrap_or_else(|| self.parser.app.version.unwrap_or(""))
        } else {
            self.parser
                .app
                .version
                .unwrap_or_else(|| self.parser.app.long_version.unwrap_or(""))
        };
        if let Some(bn) = self.parser.app.bin_name.as_ref() {
            if bn.contains(' ') {
                // Incase we're dealing with subcommands i.e. git mv is translated to git-mv
                write!(w, "{} {}", bn.replace(" ", "-"), ver)
            } else {
                write!(w, "{} {}", &self.parser.app.name[..], ver)
            }
        } else {
            write!(w, "{} {}", &self.parser.app.name[..], ver)
        }
    }
}

// Handles Args
impl<'a, 'b, 'c, 'd> HelpWriter<'a, 'b, 'c, 'd> {
    /// Writes help for each argument in the order they were declared to the wrapped stream.
    fn write_args_unsorted<'z, W: Write, I: 'z>(&mut self, w: &mut W, args: I) -> io::Result<()>
    where
        I: Iterator<Item = &'z Arg<'a, 'b>>,
        'b: 'z,
    {
        debugln!("HelpWriter::write_args_unsorted;");
        // The shortest an arg can legally be is 2 (i.e. '-x')
        self.longest = 2;
        let mut arg_v = Vec::with_capacity(10);
        for arg in args.filter(|arg| {
            !arg.is_set(ArgSettings::Hidden)
        })
        {
            let arg_str = arg.to_string();
            let arg_len = str_width(&*arg_str);
            // If it's NextLineHelp, but we don't care to compute how long because it may be
            // NextLineHelp on purpose *because* it's so long and would throw off all other
            // args alignment
            if !arg._settings.is_set(ArgSettings::NextLineHelp) {
                self.longest = cmp::max(self.longest, arg_len);
            }
            arg_v.push((arg, arg_str, arg_len));
        }
        let mut first = true;
        for (arg, arg_str, arg_len) in arg_v {
            debugln!("HelpWriter::write_args_unsorted:iter:{}:", arg.name);
            if first {
                first = false;
            } else {
                w.write_all(b"\n")?;
            }
            self.write_arg(w, arg, &*arg_str, arg_len)?;
        }
        Ok(())
    }

    /// Sorts arguments by length and display order and write their help to the wrapped stream.
    fn write_args<'z, W: Write, I: 'z>(&mut self, w: &mut W, args: I) -> io::Result<()>
    where
        I: Iterator<Item = &'z Arg<'a, 'b>>,
        'b: 'z
    {
        debugln!("HelpWriter::write_args;");
        // The shortest an arg can legally be is 2 (i.e. '-x')
        self.longest = 2;
        let ddo = self.parser.is_set(AppSettings::DeriveDisplayOrder);
        let mut ord_m = VecMap::new();
        // Determine the longest
        for arg in args.filter(|arg| {
            !arg.is_set(ArgSettings::Hidden)
        })
        {
            let arg_str = arg.to_string();
            let arg_len = str_width(&*arg_str);
            // If it's NextLineHelp, but we don't care to compute how long because it may be
            // NextLineHelp on purpose *because* it's so long and would throw off all other
            // args alignment
            if !arg._settings.is_set(ArgSettings::NextLineHelp) {
                self.longest = cmp::max(self.longest, arg_len);
            }
            let order = if ddo { arg._derived_order } else { arg.display_order };
            let btm = ord_m.entry(order).or_insert(BTreeMap::new());
            btm.insert(arg.name, (arg, arg_str, arg_len));
        }
        let mut first = true;
        for btm in ord_m.values() {
            for &(arg, ref arg_str, arg_len) in btm.values() {
                if first {
                    first = false;
                } else {
                    w.write_all(b"\n")?;
                }
                self.write_arg(w, arg, &*arg_str, arg_len)?;
            }
        }
        Ok(())
    }

    /// Writes help for an argument to the wrapped stream.
    fn write_arg<W: Write>(&mut self, w: &mut W, arg: &Arg<'a, 'b>, arg_str: &str, arg_len: usize) -> io::Result<()> {
        debugln!("HelpWriter::write_arg:{};", arg.name);
        write!(w, "{}", TAB)?;
        if arg.long.is_some() {
            self.short(w, arg, arg_str)?;
        }
        color!(self, w, "{}", arg_str, good)?;
        let spec_vals = self.spec_vals(arg);
        let h = if self.use_long {
            arg.long_help.unwrap_or_else(|| arg.help.unwrap_or(""))
        } else {
            arg.help.unwrap_or_else(|| arg.long_help.unwrap_or(""))
        };
        let h_w = str_width(h) + str_width(&*spec_vals);
        self.write_arg_spaces(w, arg, &*spec_vals, arg_len, h, h_w)?;
        self.help(w, arg, &*spec_vals, arg_len, h, h_w)?;
        Ok(())
    }

    /// Writes argument's short command to the wrapped stream.
    fn short<W: Write>(&mut self, w: &mut W, arg: &Arg<'a, 'b>, arg_str: &str) -> io::Result<()> {
        debugln!("HelpWriter::short:{};", arg.name);
        if let Some(s) = arg.short {
            if arg.long.is_some() {
                color!(self, w, "-{}, ", s, good)
            } else {
                color!(self, w, "{}", arg_str, good)
            }
        } else if arg._has_switch() {
            write!(w, "{}", TAB)
        } else {
            Ok(())
        }
    }

    /// Writes argument's help to the wrapped stream.
    fn spec_vals(&self, a: &Arg) -> String {
        debugln!("HelpWriter::spec_vals:{}", a.name);
        let mut spec_vals = vec![];
        if !a.is_set(ArgSettings::HideDefaultValue) {
            if let Some(pv) = a.default_value {
                debugln!("HelpWriter::spec_vals:{}: Found default value...[{:?}]", a.name, pv);
                spec_vals.push(format!(
                    " [default: {}]",
                    if self.color {
                        self.cizer.good(pv.to_string_lossy())
                    } else {
                        Format::None(pv.to_string_lossy())
                    }
                ));
            }
        }
        if let Some(ref aliases) = a.visible_aliases {
            debugln!("HelpWriter::spec_vals:{}: Found aliases...{:?}", a.name, aliases);
            spec_vals.push(format!(
                " [aliases: {}]",
                if self.color {
                    aliases
                        .iter()
                        .map(|v| format!("{}", self.cizer.good(v)))
                        .collect::<Vec<_>>()
                        .join(", ")
                } else {
                    aliases.join(", ")
                }
            ));
        }
        if !self.hide_pv && !a.is_set(ArgSettings::HidePossibleValues) {
            if let Some(ref pv) = a.possible_values {
                debugln!("HelpWriter::spec_vals:{}: Found possible vals...{:?}", a.name, pv);
                spec_vals.push(if self.color {
                    format!(
                        " [values: {}]",
                        pv.iter()
                            .map(|v| format!("{}", self.cizer.good(v)))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                } else {
                    format!(" [values: {}]", pv.join(", "))
                });
            }
        }
        spec_vals.join(" ")
    }

    /// Writes argument's help to the wrapped stream.
    fn help<W: Write>(&mut self, w: &mut W, arg: &Arg<'a, 'b>, spec_vals: &str, arg_len: usize, h: &str, h_w: usize) -> io::Result<()> {
        debugln!("HelpWriter::help:{};", arg.name);

        let mut help = String::from(h) + spec_vals;
        let nlh = self.next_line_help || arg.is_set(ArgSettings::NextLineHelp) || self.use_long;
        debugln!("HelpWriter::help:{}: Next Line...{:?}", arg.name, nlh);

        let spcs = if nlh || self.force_next_line {
            12 // "tab" * 3
        } else {
            self.longest + 12
        };

        let too_long = spcs + h_w >= self.term_width;

        // Is help on next line, if so then indent
        if nlh || self.force_next_line {
            write!(w, "\n{}{}{}", TAB, TAB, TAB)?;
        }

        if too_long && spcs <= self.term_width || h.contains("{n}") {
            debugln!("HelpWriter::help:{}: help width={}, too long", arg.name, str_width(&*help));
            // Determine how many newlines we need to insert
            let avail_chars = self.term_width - spcs;
            debugln!("HelpWriter::help:{}: Usable space...{}", arg.name, avail_chars);
            help = wrap_help(&help.replace("{n}", "\n"), avail_chars);
        }
        if let Some(part) = help.lines().next() {
            write!(w, "{}", part)?;
        }
        for part in help.lines().skip(1) {
            write!(w, "\n")?;
            if nlh || self.force_next_line {
                write!(w, "{}{}{}", TAB, TAB, TAB)?;
            } else if arg._has_switch() {
                write_nspaces!(w, self.longest + 12);
            } else {
                write_nspaces!(w, self.longest + 8);
            }
            write!(w, "{}", part)?;
        }
        if !help.contains('\n') && (nlh || self.force_next_line) {
            write!(w, "\n")?;
        }
        Ok(())
    }

    fn write_arg_spaces<W: Write>(&mut self, w: &mut W, arg: &Arg, spec_vals: &str, arg_len: usize, h: &str, h_w: usize) -> io::Result<()> {
        // Write sep
        let nlh = self.next_line_help || arg.is_set(ArgSettings::NextLineHelp);
        let taken = self.longest + 12;
        self.force_next_line = !nlh && self.term_width >= taken &&
            (taken as f32 / self.term_width as f32) > 0.40 &&
            h_w > (self.term_width - taken);

        if arg._has_switch() {
            if !(nlh || self.force_next_line) {
                // subtract ourself
                let mut spcs = self.longest - arg_len;
                // Since we're writing spaces from the tab point we first need to know if we
                // had a long and short, or just short
                if arg.long.is_some() {
                    // Only account 4 after the val
                    spcs += 4;
                } else {
                    // Only account for ', --' + 4 after the val
                    spcs += 8;
                }

                write_nspaces!(w, spcs);
            }
        } else if !(nlh || self.force_next_line) {
            write_nspaces!(
                w,
                self.longest + 4 - arg_len
            );
        }

        Ok(())
    }
}

// Handles Subcommands
impl<'a, 'b, 'c, 'd> HelpWriter<'a, 'b, 'c, 'd> {
    /// Writes help for subcommands of a Parser Object to the wrapped stream.
    fn write_subcommands<W: Write>(&mut self, w: &mut W) -> io::Result<()> {
        debugln!("HelpWriter::write_subcommands;");
        // The shortest an arg can legally be is 2 (i.e. '-x')
        self.longest = 2;
        let mut ord_m = VecMap::new();
        for sc in subcommands!(self.parser.app)
            .filter(|s| !s.is_set(AppSettings::Hidden))
        {
            let btm = ord_m.entry(sc.display_order).or_insert(BTreeMap::new());
            self.longest = cmp::max(self.longest, str_width(&*sc.name));
            btm.insert(sc.name.clone(), sc);
        }

        let mut first = true;
        for btm in ord_m.values() {
            for sc in btm.values() {
                if first {
                    first = false;
                } else {
                    w.write_all(b"\n")?;
                }
                self.write_subcommand_as_arg(w, sc)?;
            }
        }
        Ok(())
    }


    /// Writes help for an subcommand to the wrapped stream.
    fn write_subcommand_as_arg<W: Write>(&mut self, w: &mut W, sc: &App<'a, 'b>) -> io::Result<()> {
        debugln!("HelpWriter::write_subcommand_as_arg;");
        write!(w, "{}", TAB)?;
        color!(self, w, "{}", sc.name, good)?;
        let spec_vals = self.subcommand_spec_vals(sc);
        let h = if self.use_long {
            sc.long_about.unwrap_or_else(|| sc.about.unwrap_or(""))
        } else {
            sc.about.unwrap_or_else(|| sc.long_about.unwrap_or(""))
        };
        let h_w = str_width(h) + str_width(&*spec_vals);
        self.write_subcommand_spaces(w, sc, &*spec_vals, h_w)?;
        self.subcommand_help(w, sc, &*spec_vals, h, h_w)?;
        Ok(())
    }

    fn subcommand_spec_vals(&self, a: &App) -> String {
        debugln!("HelpWriter::spec_vals:{}", a.name);
        let mut spec_vals = vec![];
        if let Some(ref aliases) = a.visible_aliases {
            debugln!("HelpWriter::spec_vals:{}: Found aliases...{:?}", a.name, aliases);
            spec_vals.push(format!(
                " [aliases: {}]",
                if self.color {
                    aliases
                        .iter()
                        .map(|v| format!("{}", self.cizer.good(v)))
                        .collect::<Vec<_>>()
                        .join(", ")
                } else {
                    aliases.join(", ")
                }
            ));
        }
        spec_vals.join(" ")
    }

    fn write_subcommand_spaces<W: Write>(&mut self, w: &mut W, sc: &App, spec_vals: &str, h_w: usize) -> io::Result<()> {
        debugln!("HelpWriter::write_subcommand_spaces:{};", sc.name);
        // Write sep
        let nlh = self.next_line_help || sc.is_set(AppSettings::NextLineHelp) || self.use_long;
        let taken = self.longest + 8; // TAB*2
        self.force_next_line = !nlh && self.term_width >= taken &&
            (taken as f32 / self.term_width as f32) > 0.40 &&
            h_w > (self.term_width - taken);

        if !(nlh || self.force_next_line) {
                // subtract ourself
                let mut spcs = self.longest - str_width(&*sc.name) + 4;
                write_nspaces!(w, spcs);
        } else {
            write!(w, "\n{}{}{}", TAB, TAB, TAB)?;
        }

        Ok(())
    }

    fn subcommand_help<W: Write>(&mut self, w: &mut W, sc: &App<'a, 'b>, spec_vals: &str, h: &str, h_w: usize) -> io::Result<()> {
        debugln!("HelpWriter::subcommand_help:{}:;", sc.name);
        let mut help = String::from(h) + spec_vals;
        let nlh = self.next_line_help || self.force_next_line || self.use_long;

        let taken = if nlh || self.force_next_line {
            12 // "tab" * 3
        } else {
            self.longest + 8
        };

        let too_long = taken + h_w >= self.term_width;

        if too_long && taken <= self.term_width || h.contains("{n}") {
            debugln!("HelpWriter::subcommand_help:{}: help_width={}, Too long...", sc.name, h_w);
            // Determine how many newlines we need to insert
            let avail_chars = self.term_width - taken;
            debugln!("HelpWriter::subcommand_help:{}: Usable space...{}", sc.name, avail_chars);
            help = wrap_help(&help.replace("{n}", "\n"), avail_chars);
        }
        if let Some(part) = help.lines().next() {
            write!(w, "{}", part)?;
        }
        for part in help.lines().skip(1) {
            write!(w, "\n")?;
            if nlh || self.force_next_line {
                write!(w, "{}{}{}", TAB, TAB, TAB)?;
            } else {
                write_nspaces!(w, self.longest + 8);
            }
            write!(w, "{}", part)?;
        }
        if !help.contains('\n') && (nlh || self.force_next_line) {
            write!(w, "\n")?;
        }
        Ok(())
    }
}

// Public Methods to write help.
impl<'a, 'b, 'c, 'd> HelpWriter<'a, 'b, 'c, 'd> {
    /// Writes help for all arguments (options, flags, args, subcommands)
    /// including titles of a Parser Object to the wrapped stream.
    #[cfg_attr(feature = "lints", allow(useless_let_if_seq))]
    #[cfg_attr(feature = "cargo-clippy", allow(useless_let_if_seq))]
    pub fn write_all_args<W: Write>(&mut self, w: &mut W) -> ClapResult<()> {
        debugln!("HelpWriter::write_all_args;");
        let flags = self.parser.has_flags();
        let pos = positionals!(self.parser.app)
            .filter(|arg| !arg.is_set(ArgSettings::Hidden))
            .count() > 0;
        let opts = self.parser.has_opts();
        let subcmds = self.parser.has_subcommands();

        let unified_help = self.parser.is_set(AppSettings::UnifiedHelpMessage);

        let mut first = true;

        if unified_help && (flags || opts) {
            debugln!("HelpWriter::write_all_args: writing unified help");
            color!(self, w, "OPTIONS:\n", warning)?;
            let opts_flags = flags!(self.parser.app).chain(
                opts!(self.parser.app)
            );
            self.write_args(w, opts_flags)?;
            first = false;
        } else {
            if flags {
                debugln!("HelpWriter::write_all_args: writing FLAGS");
                color!(self, w, "FLAGS:\n", warning)?;
                self.write_args(w, flags!(self.parser.app))?;
                first = false;
            }
            if opts {
                if !first {
                    w.write_all(b"\n\n")?;
                }
                debugln!("HelpWriter::write_all_args: writing OPTIONS");
                color!(self, w, "OPTIONS:\n", warning)?;
                self.write_args(w, opts!(self.parser.app))?;
                first = false;
            }
        }

        if pos {
            if !first {
                w.write_all(b"\n\n")?;
            }
            debugln!("HelpWriter::write_all_args: writing ARGS");
            color!(self, w, "ARGS:\n", warning)?;
            self.write_args_unsorted(
                w,
                positionals!(self.parser.app)
            )?;
            first = false;
        }

        if subcmds {
            if !first {
                try!(w.write_all(b"\n\n"));
            }
            debugln!("HelpWriter::write_all_args: writing SUBCOMMANDS");
            try!(color!(self, w, "SUBCOMMANDS:\n", warning));
            try!(self.write_subcommands(w));
        }

        Ok(())
    }

    /// Writes version of a Parser Object to the wrapped stream.
    fn write_only_version<W: Write>(&mut self, w: &mut W) -> io::Result<()> {
        debugln!("HelpWriter::write_only_version;");
        write!(w, "{}", self.parser.app.version.unwrap_or(""))?;
        Ok(())
    }

    /// Writes binary name of a Parser Object to the wrapped stream.
    fn write_bin_name<W: Write>(&mut self, w: &mut W) -> io::Result<()> {
        debugln!("HelpWriter::write_bin_name;");
        macro_rules! write_name {
            () => {{
                let mut name = self.parser.app.name.clone();
                name = name.replace("{n}", "\n");
                color!(self, w, wrap_help(&name, self.term_width), good)?;
            }};
        }
        if let Some(bn) = self.parser.app.bin_name.as_ref() {
            if bn.contains(' ') {
                // Incase we're dealing with subcommands i.e. git mv is translated to git-mv
                color!(self, w, bn.replace(" ", "-"), good)?
            } else {
                write_name!();
            }
        } else {
            write_name!();
        }
        Ok(())
    }

    /// Writes default help for a Parser Object to the wrapped stream.
    pub fn write_default_help<W: Write>(&mut self, w: &mut W) -> ClapResult<()> {
        debugln!("HelpWriter::write_default_help;");
        if let Some(h) = self.parser.app.before_help {
            self.write_before_after_help(w, h)?;
            w.write_all(b"\n\n")?;
        }

        macro_rules! write_thing {
            ($thing:expr) => {{
                let mut owned_thing = $thing.to_owned();
                owned_thing = owned_thing.replace("{n}", "\n");
                write!(w, "{}\n",
                            wrap_help(&owned_thing, self.term_width))?
            }};
        }
        // Print the version
        self.write_bin_name(w)?;
        w.write_all(b" ")?;
        self.write_only_version(w)?;
        w.write_all(b"\n")?;
        if let Some(author) = self.parser.app.author {
            write_thing!(author)
        }
        if let Some(about) = self.parser.app.about {
            write_thing!(about)
        }

        color!(self, w, "\nUSAGE:", warning)?;
        write!(
            w,
            "\n{}{}\n\n",
            TAB,
            self.parser.create_usage_no_title(&[])
        )?;

        let flags = self.parser.has_flags();
        let pos = self.parser.has_positionals();
        let opts = self.parser.has_opts();
        let subcmds = self.parser.has_subcommands();

        if flags || opts || pos || subcmds {
            self.write_all_args(w)?;
        }

        if let Some(h) = self.parser.app.after_help {
            if flags || opts || pos || subcmds {
                w.write_all(b"\n\n")?;
            }
            self.write_before_after_help(w, h)?;
        }

        w.flush().map_err(ClapError::from)
    }

    fn write_before_after_help<W: Write>(&mut self, w: &mut W, h: &str) -> io::Result<()> {
        debugln!(
            "HelpWriter::write_before_after_help: Term width...{}",
            self.term_width
        );
        let mut help = String::from(h);
        // determine if our help fits or needs to wrap
        let too_long = str_width(h) >= self.term_width;

        if too_long || h.contains("{n}") {
            debugln!("HelpWriter::write_before_after_help: width={}, Too long...", 
                str_width(&*help)
            );
            // Determine how many newlines we need to insert
            help = wrap_help(&help.replace("{n}", "\n"), self.term_width);
        }
        write!(w, "{}", help)?;
        Ok(())
    }

}

/// Possible results for a copying function that stops when a given
/// byte was found.
enum CopyUntilResult {
    DelimiterFound(usize),
    DelimiterNotFound(usize),
    ReaderEmpty,
    ReadError(io::Error),
    WriteError(io::Error),
}

/// Copies the contents of a reader into a writer until a delimiter byte is found.
/// On success, the total number of bytes that were
/// copied from reader to writer is returned.
fn copy_until<R: Read, W: Write>(r: &mut R, w: &mut W, delimiter_byte: u8) -> CopyUntilResult {
    debugln!("copy_until;");

    let mut count = 0;
    for wb in r.bytes() {
        match wb {
            Ok(b) => {
                if b == delimiter_byte {
                    return CopyUntilResult::DelimiterFound(count);
                }
                match w.write(&[b]) {
                    Ok(c) => count += c,
                    Err(e) => return CopyUntilResult::WriteError(e),
                }
            }
            Err(e) => return CopyUntilResult::ReadError(e),
        }
    }
    if count > 0 {
        CopyUntilResult::DelimiterNotFound(count)
    } else {
        CopyUntilResult::ReaderEmpty
    }
}

/// Copies the contents of a reader into a writer until a {tag} is found,
/// copying the tag content to a buffer and returning its size.
/// In addition to errors, there are three possible outputs:
///   - `None`: The reader was consumed.
///   - `Some(Ok(0))`: No tag was captured but the reader still contains data.
///   - `Some(Ok(length>0))`: a tag with `length` was captured to the `tag_buffer`.
fn copy_and_capture<R: Read, W: Write>(
    r: &mut R,
    w: &mut W,
    tag_buffer: &mut Cursor<Vec<u8>>,
) -> Option<io::Result<usize>> {
    use self::CopyUntilResult::*;
    debugln!("copy_and_capture;");

    // Find the opening byte.
    match copy_until(r, w, b'{') {

        // The end of the reader was reached without finding the opening tag.
        // (either with or without having copied data to the writer)
        // Return None indicating that we are done.
        ReaderEmpty |
        DelimiterNotFound(_) => None,

        // Something went wrong.
        ReadError(e) | WriteError(e) => Some(Err(e)),

        // The opening byte was found.
        // (either with or without having copied data to the writer)
        DelimiterFound(_) => {

            // Lets reset the buffer first and find out how long it is.
            tag_buffer.set_position(0);
            let buffer_size = tag_buffer.get_ref().len();

            // Find the closing byte,limiting the reader to the length of the buffer.
            let mut rb = r.take(buffer_size as u64);
            match copy_until(&mut rb, tag_buffer, b'}') {

                // We were already at the end of the reader.
                // Return None indicating that we are done.
                ReaderEmpty => None,

                // The closing tag was found.
                // Return the tag_length.
                DelimiterFound(tag_length) => Some(Ok(tag_length)),

                // The end of the reader was found without finding the closing tag.
                // Write the opening byte and captured text to the writer.
                // Return 0 indicating that nothing was caputred but the reader still contains data.
                DelimiterNotFound(not_tag_length) => {
                    match w.write(b"{") {
                        Err(e) => Some(Err(e)),
                        _ => {
                            match w.write(&tag_buffer.get_ref()[0..not_tag_length]) {
                                Err(e) => Some(Err(e)),
                                _ => Some(Ok(0)),
                            }
                        }
                    }
                }

                ReadError(e) | WriteError(e) => Some(Err(e)),
            }
        }
    }
}


// Methods to write Parser help using templates.
impl<'a, 'b, 'c, 'd> HelpWriter<'a, 'b, 'c, 'd> {
    /// Write help to stream for the parser in the format defined by the template.
    ///
    /// Tags arg given inside curly brackets:
    /// Valid tags are:
    ///     * `{bin}`         - Binary name.
    ///     * `{version}`     - Version number.
    ///     * `{author}`      - Author information.
    ///     * `{usage}`       - Automatically generated or given usage string.
    ///     * `{all-args}`    - Help for all arguments (options, flags, positionals arguments,
    ///                         and subcommands) including titles.
    ///     * `{unified}`     - Unified help for options and flags.
    ///     * `{flags}`       - Help for flags.
    ///     * `{options}`     - Help for options.
    ///     * `{positionals}` - Help for positionals arguments.
    ///     * `{subcommands}` - Help for subcommands.
    ///     * `{after-help}`  - Info to be displayed after the help message.
    ///     * `{before-help}` - Info to be displayed before the help message.
    ///
    /// The template system is, on purpose, very simple. Therefore the tags have to writen
    /// in the lowercase and without spacing.
    fn write_templated_help<W: Write>(&mut self, w: &mut W, template: &str) -> ClapResult<()> {
        debugln!("HelpWriter::write_templated_help;");
        let mut tmplr = Cursor::new(&template);
        let mut tag_buf = Cursor::new(vec![0u8; 15]);

        // The strategy is to copy the template from the the reader to wrapped stream
        // until a tag is found. Depending on its value, the appropriate content is copied
        // to the wrapped stream.
        // The copy from template is then resumed, repeating this sequence until reading
        // the complete template.

        loop {
            let tag_length = match copy_and_capture(&mut tmplr, w, &mut tag_buf) {
                None => return Ok(()),
                Some(Err(e)) => return Err(ClapError::from(e)),
                Some(Ok(val)) if val > 0 => val,
                _ => continue,
            };

            debugln!("HelpWriter::write_template_help:iter: tag_buf={};", unsafe {
                String::from_utf8_unchecked(
                    tag_buf.get_ref()[0..tag_length]
                        .iter()
                        .map(|&i| i)
                        .collect::<Vec<_>>(),
                )
            });
            match &tag_buf.get_ref()[0..tag_length] {
                b"?" => {
                    w.write_all(b"Could not decode tag name")?;
                }
                b"bin" => {
                    self.write_bin_name(w)?;
                }
                b"version" => {
                    write!(
                        w,
                        "{}",
                        self.parser.app.version.unwrap_or("unknown version")
                    )?;
                }
                b"author" => {
                    write!(
                        w,
                        "{}",
                        self.parser.app.author.unwrap_or("unknown author")
                    )?;
                }
                b"about" => {
                    write!(
                        w,
                        "{}",
                        self.parser.app.about.unwrap_or("unknown about")
                    )?;
                }
                b"usage" => {
                    write!(
                        w,
                        "{}",
                        self.parser.create_usage_no_title(&[])
                    )?;
                }
                b"all-args" => {
                    self.write_all_args(w)?;
                }
                b"unified" => {
                    let opts_flags = flags!(self.parser.app).chain(
                        opts!(self.parser.app)
                    );
                    self.write_args(w, opts_flags)?;
                }
                b"flags" => {
                    self.write_args(w, flags!(self.parser.app))?;
                }
                b"options" => {
                    self.write_args(w, opts!(self.parser.app))?;
                }
                b"positionals" => {
                    self.write_args(w, positionals!(self.parser.app))?;
                }
                b"subcommands" => {
                    self.write_subcommands(w)?;
                }
                b"after-help" => {
                    write!(
                        w,
                        "{}",
                        self.parser.app.after_help.unwrap_or("unknown after-help")
                    )?;
                }
                b"before-help" => {
                    write!(
                        w,
                        "{}",
                        self.parser.app.before_help.unwrap_or("unknown before-help")
                    )?;
                }
                // Unknown tag, write it back.
                r => {
                    w.write_all(b"{")?;
                    w.write_all(r)?;
                    w.write_all(b"}")?;
                }
            }
        }
    }
}

fn wrap_help(help: &str, avail_chars: usize) -> String {
    let wrapper = textwrap::Wrapper::new(avail_chars).break_words(false);
    help.lines()
        .map(|line| wrapper.fill(line))
        .collect::<Vec<String>>()
        .join("\n")
}

#[cfg(test)]
mod test {
    use super::wrap_help;

    #[test]
    fn wrap_help_last_word() {
        let help = String::from("foo bar baz");
        assert_eq!(wrap_help(&help, 5), "foo\nbar\nbaz");
    }
}
