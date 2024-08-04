# Amber-Remix

Resources for decoding Amiga **Amberstar** data files, including
**Hippel-CoSo** sound modules.

## Building and running

Make sure that the Amiga Amberstar data files are in the `data/` subdirectory.  Their
names should be in all-caps (`AMBERDEV.UDO` etc.).

To compile and run, use the Rust `cargo` tool.

The current run modes are supported:
- `cargo run --bin demo strings`: Dump out all text strings
- `cargo run --bin demo graphics`: Shows some graphics
- `cargo run --bin demo song $X`: Plays the in-game song `${X}` (no looping)
- `cargo run --bin map_demo`: Map demo, allows walking through first-person dungeons

## Why?
I wanted a zero-stakes project to learn the basics of Rust, and this
seemed fun.  No promises as to whether this will or will not go
anywhere.

## Status

Very much WIP.  The following bits work to some extent:
- *Data*: Container format decoding is fully supported
- *Text*: String extraction seems to work
- *In-Game Songs*: Can play and debug the Hippel-CoSo songs (not the intro/outro ones)
- *Graphics*: Decoding for most graphics works (fonts, UI icons are missing, but I'm not sure I'll want to add them)
- *Maps*: Get loaded and can be traversed

The following get partially decoded but don't do anything yet
- *Game Scripts*: Bits and pieces from the map data are decoded


Only tested on a late English version of the game.

## Decoding status and documentation
- All container formats can be decoded.
- Please check the [WIP format descriptions](docs/FORMATS.org)


| File         | Decoded                             |
|--------------|-------------------------------------|
| AMBERDEV.UDO | only very partially                 |
| Amberload    |                                     |
| AUTOMAP.AMB  |                                     |
| BACKGRND.AMB | yes                                 |
| CHARDATA.AMB | partially (missing some attributes) |
| CHESTDAT.AMB |                                     |
| CODETXT.AMB  | yes                                 |
| COL_PALL.AMB | yes                                 |
| COM_BACK.AMB | yes                                 |
| EXTRO.UDO    |                                     |
| F_T_ANIM.ICN | yes, but not incorporated yet       |
| ICON_DAT.AMB | yes                                 |
| INTRO_P.UDO  |                                     |
| INTRO.UDO    |                                     |
| LABBLOCK.AMB | yes                                 |
| LAB_DATA.AMB | yes                                 |
| MAP_DATA.AMB | mostly                              |
| MAPTEXT.AMB  | yes                                 |
| MON_DATA.AMB | yes                                 |
| MON_GFX.AMB  | yes                                 |
| PARTYDAT.SAV |                                     |
| PICS80.AMB   | yes                                 |
| PUZZLE.ICN   |                                     |
| PUZZLE.TXT   |                                     |
| SAMPLEDA.IMG | yes                                 |
| TACTIC.ICN   | yes, but not handled by program yet |
| TH_LOGO.UDO  |                                     |
| WARESDAT.AMB |                                     |

## Links
- Ambermoon resources: https://github.com/Pyrdacor/Ambermoon

## Hacking
Check src/util.rs for integration with the Rust logging infrastructure.  To enable logging for a specific module,
such as `datafiles::map`, you can set:
```
RUST_LOG="warn,amber_remix::datafiles::map=info" cargo run --bin demo
```

## Acknowledgements
This work is based by documentation collected from the following
sources:
- [Pyrdacor](https://www.pyrdacor.net)'s collection of documentation for [Ambermoon](https://github.com/Pyrdacor/Ambermoon) and [Amberstar](https://github.com/Pyrdacor/Amberstar)
- [Pyrdacor](https://www.pyrdacor.net)'s collection of AmberMoon documentation: [https://github.com/Pyrdacor/Ambermoon]
- Heikki Orsila's [UADE](https://zakalwe.fi/uade/), specifically m68k assembly implementations of Jochen Hippel's sound formats
- Christian Corti's [Flod 4.1 player for Jochen Hippel's 4-voice formats](https://github.com/photonstorm/Flod/blob/master/Flod%204.1/neoart/flod/hippel/JHPlayer.as)
- Jurie Horneman for the [commented Atari ST Ambermoon assembly code](https://github.com/jhorneman/amberstar)
