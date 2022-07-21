// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

//#[allow(unused)]
//use log::{Level, log_enabled, trace, debug, info, warn, error};

//use rustyline::error::ReadlineError;
use rustyline::{Editor, Result};
use crate::{audio::{self, Mixer}, datafiles};

// ================================================================================
// CLI commands

enum CmdInfo {
    Cmd(Command),
    Section(& 'static str),
}

struct Command {
    n : &'static str,
    d : &'static str,
    f : fn() -> (),
}

impl Command {
    pub fn len(&self) -> usize {
	return self.n.len();
    }

    pub fn matches(&self, s : &'_ str) -> bool {
	return s == self.n;
    }

    pub fn run(&self) {
	(self.f)();
    }
}

const COMMANDS : [CmdInfo; 4] = [
    CmdInfo::Section("System commands"),

    CmdInfo::Cmd(Command {
	n : "help",
	f : cmd_help,
	d : "Print basic introductory help" }),

    CmdInfo::Cmd(Command {
	n : "list",
	f : cmd_list,
	d : "List all commands" }),

    CmdInfo::Cmd(Command {
	n : "quit",
	f : cmd_help,
	d : "Quit the program" }),
];

// ----------------------------------------
// Commands list

fn cmd_help() {
    println!("Debugger interface");
    println!("- 'list' lists all available commands");
    println!("- 'quit' quits (as do Ctrl-C and Ctrl-D)");
}

fn cmd_quit() {
    std::process::exit(0);
}

fn cmd_list() {
    let mut maxlen = 0;

    for c in COMMANDS {
	if let CmdInfo::Cmd(cmd) = c {
	    maxlen = usize::max(maxlen, cmd.len());
	}
    }

    let pad = maxlen + 4;

    for c in COMMANDS {
	match c {
	    CmdInfo::Cmd(cmd)   => println!("  {:2$}{}", cmd.n, cmd.d, pad),
	    CmdInfo::Section(s) => println!("---- {s}"),
	}
    }
}


// ================================================================================
// CLI implementation

// struct Arg<'a> {
//     str : &'a str,
// }

// impl Arg<'a> {
    
// }

struct CLI<'a> {
    data : &'a datafiles::AmberStarFiles,
    mixer : Mixer,
}

impl<'a> CLI<'a> {
    pub fn run(&mut self, line : String) {
	let mut tokens = line.split_whitespace();
	if let Some(first_token) = tokens.next() {
	    for c in COMMANDS {
		if let CmdInfo::Cmd(cmd) = c {
		    if cmd.matches(first_token) {
			cmd.run();
		    }
		}
	    }
	}
    }
}

pub fn debug_audio(data : &datafiles::AmberStarFiles) -> Result<()> {
    let sdl_context = sdl2::init().unwrap();

    let mut audiocore = audio::init(&sdl_context);
    let mut cli = CLI {
	data,
	mixer : audiocore.start_mixer(&data.sample_data.data[..]),
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
