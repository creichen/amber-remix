// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

//#[allow(unused)]
//use log::{Level, log_enabled, trace, debug, info, warn, error};

//use rustyline::error::ReadlineError;
use rustyline::Editor;
use std::{result::Result, sync::Arc, ops::Index, str::FromStr, rc::Rc};
use crate::{audio::{self, Mixer, amber, pcmplay::{StereoPCM, MonoPCM}, dsp::channel_sequencer::{AudioStream, ASMeta, ASAnnotation}}, datafiles};

// ================================================================================
// CLI commands

enum CmdInfo {
    Cmd(Command),
    Section(& 'static str),
}

struct Command {
    n : &'static str,
    a : &'static [(&'static str, FA)], // formal args
    oa : &'static [(&'static str, FA, &'static str)], // named optional args
    d : &'static str,
    f : fn(&mut CLI, &Args) -> (),
}

impl Command {
    // Length of name-plus-arguments string
    pub fn len(&self) -> usize {
	return self.to_string().len();
    }

    pub fn matches(&self, s : &'_ str) -> bool {
	return s == self.n;
    }

    pub fn run(&self, cli : &mut CLI, args : &Args) {
	(self.f)(cli, args);
    }

    pub fn to_string(&self) -> String {
	let mut s = format!("{} ", self.n);
	for (aname, aty) in self.a {
	    s = format!("{s} <{aname}:{}>", aty.str());
	}
	return s;
    }

    pub fn find_optarg<'a>(&self, name : &'a str) -> Option<FA> {
	for (argname, argty, _) in self.oa {
	    if name == *argname {
		return Some(*argty);
	    }
	}
	return None;
    }
}

const FA_TYPE_U_STR : &str = "num";
const FA_TYPE_S_STR : &str = "str";
const FA_TYPE_O_STR : &str = "offset";

#[derive(Clone, Copy)]
enum FA {
    U,
    S,
    O,
}

#[derive(Clone)]
enum AA {
    U(usize),
    S(String),
    O(AudioStreamIterator),
    Missing,
}

const COMMANDS : [CmdInfo; 11] = [
    CmdInfo::Section("System commands"),

    CmdInfo::Cmd(Command {
	n : "echo",
	f : cmd_echo, // added to shut up the various "unused" warnings
	a : &[("MESSAGE", FA::S)],
	oa : &[],
	d : "Echoes the specified message" }),

    CmdInfo::Cmd(Command {
	n : "exit",
	f : cmd_quit,
	a : &[],
	oa : &[],
	d : "Quit the program" }),

    CmdInfo::Cmd(Command {
	n : "help",
	f : cmd_help,
	a : &[],
	oa : &[],
	d : "Print basic introductory help" }),

    CmdInfo::Cmd(Command {
	n : "list",
	f : cmd_list,
	a : &[],
	oa : &[],
	d : "List all commands" }),

    CmdInfo::Section("Investigating songs"),

    CmdInfo::Cmd(Command {
	n : "song",
	f : cmd_set_song,
	a : &[("SONGNR", FA::U)],
	oa : &[
	    ("linear-crossfade", FA::U, "Number of samples to apply linear crossfade to between ticks (default: 0)"),
	    ("oversample", FA::U, "Powers of two to oversample (compensated via hermite downsampling) (default: 0)"),
	    ("maxtick", FA::U, "Maximum tick to track"),
	],
	d : "Sets the current song to debug" }),

    CmdInfo::Cmd(Command {
	n : "show",
	f : cmd_show,
	a : &[("OFFSET", FA::O)],
	oa : &[
	    ("warn", FA::U, "Threshold (percent) difference between adjacent samples at which to print warning marker (default 25)"),
	],
	d : "Show the specified offset range in the current song" }),

    CmdInfo::Cmd(Command {
	n : "play",
	f : cmd_play,
	a : &[("OFFSET", FA::O)],
	oa : &[
	],
	d : "Plays the specified channel / range" }),

    CmdInfo::Cmd(Command {
	n : "plays",
	f : cmd_plays,
	a : &[],
	oa : &[
	    ("l", FA::O, "Left audio (can be specified more than once)"),
	    ("r", FA::O, "Right audio (can be specified more than once)"),
	    ("c", FA::O, "Left+right sides (can be specified more than once)"),
	],
	d : "Plays the specified channel / range" }),

    CmdInfo::Cmd(Command {
	n : "write",
	f : cmd_write,
	a : &[("OFFSET", FA::O),
	      ("FILENAME", FA::S)],
	oa : &[
	],
	d : "Writes the specified file / range into a WAV file" }),
];

// ----------------------------------------
// Command implementations

// --------------------
fn cmd_echo(_cli: &mut CLI, args : &Args) {
    println!("{}", args[0].s());
}

// --------------------
fn cmd_help(_cli: &mut CLI, _ : &Args) {
    println!("Debugger interface");
    println!("- 'quit' quits (as do Ctrl-C and Ctrl-D)");
    println!("- 'list' lists all available commands");
    println!("Commands may require arguments.  The following types are supported:");
    println!("    {FA_TYPE_U_STR}:\tnatural numbers");
    println!("    {FA_TYPE_S_STR}:\tany string");
    println!("    {FA_TYPE_O_STR}:\tstream channel and offset of the form:  #<chan>[t<tick>][p<pos>]  , possibly followed by ellipsis '..', '..<end>' or '..+<len>'.");
    println!("\t\t  Examples:");
    println!("\t\t\t#0\t\tChannel 0 in its entirety");
    println!("\t\t\t#0t4\t\tChannel 0, tick 4");
    println!("\t\t\t#0t4..\t\tChannel 0, tick 4 and all following");
    println!("\t\t\t#0t4..6\t\tChannel 0, ticks 4..5");
    println!("\t\t\t#0t4..+2\tChannel 0, ticks 4..5 (relative notation)");
    println!("\t\t\t#0t4p5..10\tChannel 0, tick 4, offsets 5..9 relative to that tick start");
    println!("\t\t\t#0p5..10\tChannel 0, offsets 5..9");
    println!();
    println!("Commands may also take optional keyword arguments, e.g. 'foo:num', specified AFTER the mandatory arguments.");
}

// --------------------
fn cmd_quit(_cli: &mut CLI, _ : &Args) {
    std::process::exit(0);
}

// --------------------
fn cmd_list(_cli: &mut CLI, _ : &Args) {
    let mut maxlen = 0;

    for c in COMMANDS {
	if let CmdInfo::Cmd(cmd) = c {
	    maxlen = usize::max(maxlen, cmd.len());
	}
    }

    let pad = maxlen + 4;

    for c in COMMANDS {
	match c {
	    CmdInfo::Cmd(cmd)   => { println!("  {:2$}{}", cmd.to_string(), cmd.d, pad);
	                             for (n, ty, descr) in cmd.oa {
					 let lhs = format!("{n}:{}", ty.str());
					 println!("      {lhs:<20}    {descr}");
				     }
	                           },
	    CmdInfo::Section(s) => println!("---- {s}"),
	}
    }
}

// --------------------
fn cmd_set_song(cli: &mut CLI, args : &Args) {
    let song_nr = args[0].u();
    if song_nr >= cli.data.songs.len() {
	println!("Bad song number: 0..{}", cli.data.songs.len() - 1);
	return;
    }
    let songit = cli.get_song(song_nr);
    let mut maxtick = None;
    if args.has_opt("maxtick") {
	maxtick = Some(args.get_opt("maxtick").u());
    }
    cli.songinfo = format!("Song {}", song_nr);
    cli.song_nr = Some(song_nr);
    let channels = audio::dsp::channel_sequencer::sequence_sinc_linear(songit, cli.mixer.get_freq(),
								       maxtick,
								       args.get_opt("oversample").default_u(0),
								       args.get_opt("linear-crossfade").default_u(0)
    );
    cli.channels = vec![];
    for c in channels {
	cli.channels.push(Rc::new(c));
    }
}

// --------------------
fn cmd_show(_cli: &mut CLI, args : &Args) {
    let it = args[0].offset();

    let warn_threshold = args.get_opt("warn").default_u(25) as f32 * 0.02;

    let mut must_newline = true;
    let mut count = 0;
    const MAX_COUNT : usize = 50;
    let mut last_sample = None;
    for (pos, sample, meta) in it {
	if count >= MAX_COUNT {
	    must_newline = true;
	}
	if meta.len() > 0 {
	    must_newline = true;
	}

	if must_newline {
	    if count > 0 {
		println!();
	    }
	    count = 0;
	}

	if meta.len() > 0 {
	    for m in meta {
		match m {
		    ASMeta::A(ASAnnotation { subsys, event, data })  => println!("-- [{subsys}] {event} {data}"),
		    ASMeta::TS(t)                                    => println!("--- tick t{t}"),
		}}
	}

	if must_newline {
	    print!("{:<7}: ", pos);
	    must_newline = false;
	}

	let s = if sample < 0.0 {
	    format!("-{}", (-(sample * 128.0)) as usize)
	} else if sample > 0.0 {
	    format!("+{}", ( (sample * 127.0)) as usize)
	} else {
	    " 0".to_string()
	};
	if last_sample.is_some() && f32::abs(last_sample.unwrap() - sample) > warn_threshold {
	    print!("\x1b[41;33m!\x1b[0m{:>3}", s);
	} else {
	    print!(" {:>3}", s);
	}
	count += 1;

	last_sample = Some(sample);
    }
    println!();
}

// --------------------
fn cmd_play(cli: &mut CLI, args : &Args) {
    let offset = args[0].offset();
    cli.mixer.play_pcm(StereoPCM::from(MonoPCM::from(offset)));
}

// --------------------
fn cmd_plays(cli: &mut CLI, args : &Args) {
    for l in args.get_opts("l") {
	cli.mixer.play_pcm(MonoPCM::from(l.offset()).to_stereo(1.0, 0.0));
    }
    for r in args.get_opts("r") {
	cli.mixer.play_pcm(MonoPCM::from(r.offset()).to_stereo(0.0, 1.0));
    }
    for c in args.get_opts("c") {
	cli.mixer.play_pcm(MonoPCM::from(c.offset()).to_stereo(1.0, 1.0));
    }
}

// --------------------
fn cmd_write(cli: &mut CLI, args : &Args) {
    let offset = args[0].offset();
    let filename = args[1].s();
    let spec = hound::WavSpec {
	channels: 1,
	sample_rate: cli.mixer.get_freq() as u32,
	bits_per_sample: 32,
	sample_format : hound::SampleFormat::Float,
    };
    match hound::WavWriter::create(filename, spec) {
	Err(s) => { println!("Error: {}", s); return; }
	Ok(mut writer) => {
	    for (_, s, _) in offset {
		if let Err(e) = writer.write_sample(s) {
		    println!("Error while writing: {e}");
		    return;
		}
	    }
	}
    }
}

// ================================================================================
// CLI implementation

// ----------------------------------------
// Audio offset

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub(super) enum AOffsetEllipsis {
    Rest, // All the rest
    Relative(usize),
    Absolute(usize),
}

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub(super) struct AOffset {
    channel : usize,
    tick : Option<usize>,
    start : Option<usize>,
    ellipsis : AOffsetEllipsis,
}

impl AOffset {
    pub fn new(channel : usize, tick : Option<usize>, start : Option<usize>, ellipsis : AOffsetEllipsis) -> AOffset {
	return AOffset { channel, tick, start, ellipsis };
    }

    pub fn try_iter<'a>(&self, stream : Rc<AudioStream<ASMeta>>) -> Result<AudioStreamIterator, String> {
	let start_pos = self.start.unwrap_or(0);
	let start_tick = match self.tick {
	    None    => 0,
	    Some(t) => match stream.find_tick(t) {
		           Some(o) => o,
		           None    => { return Err(format!("Start tick {t} not found")); },
	               },
	};

	let mut end = stream.audio.len(); // to the end

	if self.start.is_none() {
	    // Ellipsis refers to the tick
	    if let Some(endtick) = match self.ellipsis {
		AOffsetEllipsis::Rest        => None,
		AOffsetEllipsis::Relative(n) => Some(n + self.tick.unwrap()), // parser ensures that this is Some
		AOffsetEllipsis::Absolute(n) => Some(n),
	    } { if let Some(e)  = stream.find_tick(endtick) {
		end = e;
	      } else {
		println!("End tick {endtick} not found, defaulting to end of channel stream");
	    }}
	} else {
	    // Ellipsis refers to the position
	    if let Some(endpos) = match self.ellipsis {
		AOffsetEllipsis::Rest        => None,
		AOffsetEllipsis::Relative(n) => Some(start_pos + n),
		AOffsetEllipsis::Absolute(n) => Some(n),
	    } { end = endpos; }
	}
	let start = start_tick + start_pos;

	return Ok(AudioStreamIterator::new(stream, start, end));
    }
}

/// Returns (number, destoffset) on success
pub(super) fn ascii_digit_slice(s : &[u8], start : usize) -> Option<(usize, usize)> {
    let mut end = start;
    while end < s.len() && s[end].is_ascii_digit() {
	end += 1;
    }
    if end > start {
	let number_str = match std::str::from_utf8(&s[start..end]) {
	    Ok(v)  => v,
	    Err(_) => return None,
	};
	let number = match str::parse::<usize>(number_str) {
	    Ok(n)  => n,
	    Err(_) => return None,
	};
	return Some((number, end));
    }
    return None;
}

impl FromStr for AOffset {
    type Err = &'static str;

    fn from_str(raw_s: &str) -> Result<Self, Self::Err> {
	let s = raw_s.as_bytes();
	let end = s.len();

	if s.len() < 2 || s[0] as char != '#' {
	    return Err("Must start with channel ID, e.g., '#0'");
	}

	if let Some((channel, next)) = ascii_digit_slice(s, 1) {
	    if next == end {
		return Ok(AOffset::new(channel, None, None, AOffsetEllipsis::Rest));
	    }

	    if s[next] as char == '.' {
		return Err("Ellipsis not allowed for channel numbers")
	    }

	    let (tick, next) = if s[next] as char == 't' {
		if let Some((tick_nr, next)) = ascii_digit_slice(s, next + 1) {
		    (Some(tick_nr), next)
		} else {
		    return Err("Channel tick must be a number, e.g., '#0t42'")
		}
	    } else { (None, next) };

	    if next == end {
		return Ok(AOffset::new(channel, tick, None, AOffsetEllipsis::Relative(1)));
	    }

	    let (start, next) =
		if s[next] as char == 'p'  {
		    if let Some((start, next)) = ascii_digit_slice(s, next + 1) {
			(Some(start), next)
		    } else { return Err("Channel position must be a number, e.g., '#0p7'") }
		} else { (None, next) };

	    let mut next = next;
	    let ellipsis = if next + 2 <= end
		&& s[next] as char == '.'
		&& s[next + 1] as char == '.' {
		    next += 2;
		    true
		} else { false };

	    if next == end {
		return Ok(AOffset::new(channel, tick, start, if ellipsis { AOffsetEllipsis::Rest } else { AOffsetEllipsis::Relative(1) }));
	    } else if !ellipsis {
		return Err("Channel position ill-formed");
	    }

	    let relative = if s[next] as char == '+' {
		next += 1;
		true
	    } else { false };

	    if next == end {
		return Err("Channel position ill-formed: trailing '..+'");
	    }

	    if let Some((offset, next)) = ascii_digit_slice(s, next) {
		if next == end {
		    return Ok(AOffset::new(channel, tick, start, if relative { AOffsetEllipsis::Relative(offset) } else { AOffsetEllipsis::Absolute(offset) }));
		}
	    }
	    return Err("Channel position ill-formed: Ellipsis must end with number");
	} else {
	     return Err("Must start with channel ID, e.g., '#0'");
	}
    }
}

// ----------------------------------------
// Tests
#[cfg(test)]
mod test {
    use crate::debug_audio::ascii_digit_slice;
    use super::AOffset;
    use super::AOffsetEllipsis::*;

    fn expect_fail(outcome : Result<AOffset, &'static str>) {
	if let Ok(_) = outcome {
	    panic!("Unexpected success: {:?}", outcome);
	}
    }

    fn expect(r : AOffset, outcome : Result<AOffset, &'static str>) {
	if let Ok(r2) = outcome {
	    assert_eq!(r, r2);
	} else {
	    panic!("Unexpected failure: {:?}", outcome);
	}
    }

    #[test]
    pub fn test_ascii_digit_slice_success() {
	assert_eq!(Some((0, 1)), ascii_digit_slice("0".as_bytes(), 0));
	assert_eq!(Some((9, 1)), ascii_digit_slice("9".as_bytes(), 0));
	assert_eq!(Some((11, 2)), ascii_digit_slice("11".as_bytes(), 0));
	assert_eq!(Some((0, 2)), ascii_digit_slice("z0".as_bytes(), 1));
	assert_eq!(Some((11, 3)), ascii_digit_slice("z11".as_bytes(), 1));

	assert_eq!(Some((0, 1)), ascii_digit_slice("0P".as_bytes(), 0));
	assert_eq!(Some((11, 2)), ascii_digit_slice("11P".as_bytes(), 0));
	assert_eq!(Some((0, 2)), ascii_digit_slice("z0P".as_bytes(), 1));
	assert_eq!(Some((11, 3)), ascii_digit_slice("z11P".as_bytes(), 1));
    }

    #[test]
    pub fn test_ascii_digit_slice_fail() {
	assert_eq!(None, ascii_digit_slice("0".as_bytes(), 1));
	assert_eq!(None, ascii_digit_slice("z".as_bytes(), 0));
	assert_eq!(None, ascii_digit_slice("0".as_bytes(), 27));
	assert_eq!(None, ascii_digit_slice("0a2".as_bytes(), 1));
    }

    #[test]
    pub fn test_parse_aoffset_chan() {
	expect(AOffset::new(3, None, None, Rest),
	       str::parse::<AOffset>("#3"));
	expect(AOffset::new(13, None, None, Rest),
	       str::parse::<AOffset>("#13"));
    }

    #[test]
    pub fn test_parse_aoffset_channel_bad() {
	expect_fail(str::parse::<AOffset>(""));
	expect_fail(str::parse::<AOffset>("#"));
	expect_fail(str::parse::<AOffset>("7"));
	expect_fail(str::parse::<AOffset>("*7"));
    }

    #[test]
    pub fn test_parse_aoffset_tick() {
	expect(AOffset::new(2, Some(1), None, Relative(1)),
	       str::parse::<AOffset>("#2t1"));
	expect(AOffset::new(2, Some(17), None, Relative(1)),
	       str::parse::<AOffset>("#2t17"));
    }

    #[test]
    pub fn test_parse_aoffset_tick_ellipsis() {
	expect(AOffset::new(2, Some(1), None, Absolute(2)),
	       str::parse::<AOffset>("#2t1..2"));
	expect(AOffset::new(2, Some(17), None, Absolute(33)),
	       str::parse::<AOffset>("#2t17..33"));
	expect(AOffset::new(2, Some(1), None, Relative(2)),
	       str::parse::<AOffset>("#2t1..+2"));
	expect(AOffset::new(2, Some(17), None, Relative(33)),
	       str::parse::<AOffset>("#2t17..+33"));
	expect(AOffset::new(2, Some(1), None, Rest),
	       str::parse::<AOffset>("#2t1.."));
	expect(AOffset::new(2, Some(17), None, Rest),
	       str::parse::<AOffset>("#2t17.."));
    }

    #[test]
    pub fn test_parse_aoffset_tick_pos() {
	expect(AOffset::new(2, Some(1), Some(13), Relative(1)),
	       str::parse::<AOffset>("#2t1p13"));
	expect(AOffset::new(2, Some(17), Some(2), Relative(1)),
	       str::parse::<AOffset>("#2t17p2"));
    }

    #[test]
    pub fn test_parse_aoffset_tick_pos_ellipsis() {
	expect(AOffset::new(2, Some(1), Some(3), Absolute(2)),
	       str::parse::<AOffset>("#2t1p3..2"));
	expect(AOffset::new(2, Some(17), Some(999), Absolute(33)),
	       str::parse::<AOffset>("#2t17p999..33"));
	expect(AOffset::new(2, Some(1), Some(8), Relative(2)),
	       str::parse::<AOffset>("#2t1p8..+2"));
	expect(AOffset::new(2, Some(17), Some(128), Relative(33)),
	       str::parse::<AOffset>("#2t17p128..+33"));
	expect(AOffset::new(2, Some(1), Some(3), Rest),
	       str::parse::<AOffset>("#2t1p3.."));
	expect(AOffset::new(2, Some(17), Some(999), Rest),
	       str::parse::<AOffset>("#2t17p999.."));
    }

    #[test]
    pub fn test_parse_aoffset_pos() {
	expect(AOffset::new(2, None, Some(1), Relative(1)),
	       str::parse::<AOffset>("#2p1"));
	expect(AOffset::new(2, None, Some(17), Relative(1)),
	       str::parse::<AOffset>("#2p17"));
    }

    #[test]
    pub fn test_parse_aoffset_pos_ellipsis() {
	expect(AOffset::new(2, None, Some(1), Absolute(2)),
	       str::parse::<AOffset>("#2p1..2"));
	expect(AOffset::new(2, None, Some(17), Absolute(33)),
	       str::parse::<AOffset>("#2p17..33"));
	expect(AOffset::new(2, None, Some(1), Relative(2)),
	       str::parse::<AOffset>("#2p1..+2"));
	expect(AOffset::new(2, None, Some(17), Relative(33)),
	       str::parse::<AOffset>("#2p17..+33"));
	expect(AOffset::new(2, None, Some(1), Rest),
	       str::parse::<AOffset>("#2p1.."));
	expect(AOffset::new(2, None, Some(17), Rest),
	       str::parse::<AOffset>("#2p17.."));
    }

    #[test]
    pub fn test_parse_aoffset_basic_bad() {
	expect_fail(str::parse::<AOffset>("#0t"));
	expect_fail(str::parse::<AOffset>("#0p"));
	expect_fail(str::parse::<AOffset>("#0p1t2"));
	expect_fail(str::parse::<AOffset>("#0ta"));
	expect_fail(str::parse::<AOffset>("#0tp"));
	expect_fail(str::parse::<AOffset>("#0pt"));
	expect_fail(str::parse::<AOffset>("#0pz"));
	expect_fail(str::parse::<AOffset>("#0t."));
	expect_fail(str::parse::<AOffset>("#0p."));
    }


    #[test]
    pub fn test_parse_aoffset_ellipsis_bad() {
	expect_fail(str::parse::<AOffset>("#0t1..+"));
	expect_fail(str::parse::<AOffset>("#0t1p2..+"));
	expect_fail(str::parse::<AOffset>("#0p1..+"));

	expect_fail(str::parse::<AOffset>("#0t1..."));
	expect_fail(str::parse::<AOffset>("#0t1p2..."));
	expect_fail(str::parse::<AOffset>("#0p1..."));

	expect_fail(str::parse::<AOffset>("#0t1."));
	expect_fail(str::parse::<AOffset>("#0t1p2."));
	expect_fail(str::parse::<AOffset>("#0p1."));

	expect_fail(str::parse::<AOffset>("#0t1..+z"));
	expect_fail(str::parse::<AOffset>("#0t1p2..+z"));
	expect_fail(str::parse::<AOffset>("#0p1..+z"));

	expect_fail(str::parse::<AOffset>("#0.."));
	expect_fail(str::parse::<AOffset>("#0..+1"));
	expect_fail(str::parse::<AOffset>("#0..1"));
    }

}

// ----------------------------------------
// AudioStreamIterator

#[derive(Clone)]
pub struct AudioStreamIterator {
    stream : Rc<AudioStream<ASMeta>>,
    pos : usize,
    it_pos : usize, // next position, if inside
    meta_index : usize,
    end_pos : usize,
}

impl AudioStreamIterator {
    pub fn new(stream : Rc<AudioStream<ASMeta>>, start_pos : usize, end_pos : usize) -> AudioStreamIterator {
	AudioStreamIterator {
	    stream : stream.clone(),
	    pos : start_pos,
	    it_pos : start_pos,
	    meta_index : 0,
	    end_pos,
	}
    }

    /// Retrieves all meta-information for the current iterator index
    pub fn meta(&mut self) -> Vec<ASMeta> {
	let pos_for_meta = self.pos;
	while self.meta_index < self.stream.meta.len() && self.stream.meta[self.meta_index].0 < pos_for_meta {
	    self.meta_index += 1;
	}

	let mut results = vec![];
	let mut i = self.meta_index;
	while i < self.stream.meta.len() && self.stream.meta[i].0 == pos_for_meta {
	    results.push(self.stream.meta[i].1.clone());
	    i += 1;
	}
	return results;
    }

    pub fn pcm(&self) -> Vec<f32> {
	return self.stream.audio[self.pos..self.end_pos].to_vec();
    }
}

impl Iterator for AudioStreamIterator {
    type Item = (usize, f32, Vec<ASMeta>);

    fn next(&mut self) -> Option<Self::Item> {
	self.pos = self.it_pos;
	if self.it_pos < self.end_pos {
	    self.it_pos += 1;
	    return Some((self.pos, self.stream.audio[self.pos], self.meta()));
	} else {
	    return None;
	}
    }
}

impl From<AudioStreamIterator> for MonoPCM {
    fn from(astreamit : AudioStreamIterator) -> Self {
	return MonoPCM::from(astreamit.pcm());
    }
}

// ----------------------------------------
// Arguments

struct Args {
    kwargs : Vec<(String, AA)>,
    posargs : Vec<AA>,
}

impl Args {
    /// Set value of optional parameter
    pub fn set_optional<'a>(&mut self, s : &'a str, value : AA) {
	self.kwargs.push((s.to_string(), value));
    }

    pub fn has_opt<'a>(&self, argname : &'a str) -> bool {
	if let AA::Missing = self.get_opt(argname) {
	    return false;
	}
	return true;
    }

    pub fn get_opt<'a>(&self, argname : &'a str) -> AA {
	for (n, v) in &self.kwargs {
	    if n == argname {
		return v.clone();
	    }
	}
	return AA::Missing;
    }

    pub fn get_opts<'a>(&self, argname : &'a str) -> Vec<AA> {
	let mut results = vec![];
	for (n, v) in &self.kwargs {
	    if n == argname {
		results.push(v.clone());
	    }
	}
	return results;
    }
}

impl Index<usize> for Args {
    type Output = AA;

    fn index(&self, index: usize) -> &Self::Output {
	return &self.posargs[index];
    }
}

impl AA {
    pub fn u(&self) -> usize {
	return if let AA::U(v) = self { *v } else { panic!("Unexpected type"); };
    }

    pub fn s(&self) -> String {
	return if let AA::S(s) = self { s.clone() } else { "<unexpected type>".to_string() };
    }

    pub fn offset(&self) -> AudioStreamIterator {
	if let AA::O(offset) = self { offset.clone() } else { panic!("Unexpected type"); }
    }

    pub fn default_u(&self, default : usize) -> usize {
	if let AA::Missing = self {
	    return default;
	} else {
	    return self.u();
	}
    }
}


impl FA {
    pub fn convert<'a>(&self, cli : &'a CLI, s : &'a str) -> Result<AA, String> {
	match self {
	    FA::U => match str::parse::<usize>(s) {
		         Ok(v)  => Ok(AA::U(v)),
		         Err(_) => Err(format!("Could not parse '{s}' as number")),
	             },
	    FA::O => match str::parse::<AOffset>(s) {
		         Ok(o)  => { if o.channel >= cli.channels.len() {
			                 Err(format!("Invalid channel number: We have {} channels in {}", cli.channels.len(), cli.songinfo))
			           } else {
			                 match o.try_iter(cli.channels[o.channel].clone()) {
					     Err(s)   => Err(s),
					     Ok(iter) => Ok(AA::O(iter)),
					 } }
			           },
		         Err(e) => Err(format!("Could not parse '{s}' as offset: {e}")),
	             },
	    FA::S => Result::Ok(AA::S(s.to_string())),
	}
    }

    pub fn str(&self) -> &'static str {
	match self {
	    FA::U => FA_TYPE_U_STR,
	    FA::S => FA_TYPE_S_STR,
	    FA::O => FA_TYPE_O_STR,
	}
    }
}

struct CLI<'a> {
    data : &'a datafiles::AmberStarFiles,
    samples : Arc<Vec<i8>>,
    mixer : Mixer,
    songinfo : String,
    song_nr : Option<usize>,
    channels : Vec<Rc<AudioStream<ASMeta>>>,
}

fn parse_colonpair<'a>(s : &'a str) -> Option<(&'a str, &'a str)> {
    let tokens : Vec<&'a str> = s.split(":").collect();
    if tokens.len() == 2 {
	return Some((tokens[0], tokens[1]));
    }
    return None
}

impl<'a> CLI<'a> {

    pub fn get_song(&self, song_id : usize) -> audio::iterator::ArcPoly {
	let songit = amber::play_song(&self.data.songs[song_id]);
	let mut guard = songit.lock().unwrap();
	guard.set_default_samples(self.samples.clone());
	return songit.clone();
    }

    pub fn run(&mut self, line : String) {
	let mut tokens = line.split_whitespace();
	if let Some(first_token) = tokens.next() {
	    for c in COMMANDS {
		if let CmdInfo::Cmd(cmd) = c {
		    if cmd.matches(first_token) {
			self.try_run(&cmd, &mut tokens);
			return;
		    }
		}
	    }
	}
    }

    fn try_run(&mut self, cmd : &Command, tokens : &mut std::str::SplitWhitespace) {
	let mut actuals = vec![];
	let mut failed = false;

	for (formal_name, formal) in cmd.a.iter() {
	    match tokens.next() {
		None     => { failed = true;
			      println!("Not enough arguments");
			      break;
		            }
		Some(s)  => match formal.convert(self, s) {
		              Ok(a)  => actuals.push(a),
		              Err(m) => { println!("{formal_name}: {}", m);
					  failed = true; }
		}
	    }
	}

	let mut args = Args { posargs : actuals, kwargs : vec![], };

	// Find optional arguments
	while !failed {
	    if let Some(optarg_candidate) = tokens.next() {
		match parse_colonpair(optarg_candidate) {
		    Some((l, r)) => match cmd.find_optarg(l) {
			              Some(t) => match t.convert(self, r) {
					           Ok(aa) => args.set_optional(l, aa),
					           Err(s) => { failed = true;
							       println!("Command '{}': argument '{l}': {s}", cmd.n);
						             },
				                 },
			              None    => { failed = true;
					           println!("Command '{}' does not know optional argument '{l}'", cmd.n);
				                 },
		                  },
		    None       => { failed = true;
				    println!("Too many arguments, expected {}", cmd.a.len());
		                  }

		}
	    } else {
		break;
	    }
	}

	if !failed {
	    cmd.run(self, &args);
	}
    }
}

pub fn debug_audio(data : &datafiles::AmberStarFiles) -> rustyline::Result<()> {
    let sdl_context = sdl2::init().unwrap();

    let mut audiocore = audio::init(&sdl_context);
    let mut cli = CLI {
	data,
	samples : Arc::new(data.sample_data.data[..].to_vec()),
	mixer : audiocore.start_mixer(&data.sample_data.data[..]),
	songinfo : "<no song>".to_string(),
	song_nr : None,
	channels : vec![],
    };

    // `()` can be used when no completer is required
    let mut rl = Editor::<()>::new();
    rl.load_history(".amber-remix-debug-audio-history").unwrap_or(());
    loop {
        let readline = rl.readline("> ");
        match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str());
		cli.run(line);
            },
            Err(_) => break,
        }
    }
    rl.save_history(".amber-remix-debug-audio-history")
}
