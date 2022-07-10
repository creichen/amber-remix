# Amber-Remix

Resources for decoding Amiga **AmberStar** data files, including
**Hippel-CoSo** sound modules.

## Building and running

Make sure that the Amiga AmberStar data files are in the `data/` subdirectory.  Their
names should be in all-caps (`AMBERDEV.UDO` etc.).

To compile and run, use the Rust `cargo` tool.

The current run modes are supported:
- `cargo run strings`: Dump out all text strings
- `cargo run graphics`: Shows some graphics
- `cargo run song $X`: Tries to play in-game song `${X}` (will likely crash sooner or later).

## Why?
I wanted a zero-stakes project to learn the basics of Rust, and this
seemed fun.  No promises as to whether this will or will not go
anywhere.

## Status

Very much WIP.  The following bits work to some extent:
- *Data*: Container format decoding is fully supported
- *Text*: String extraction seems to work
- *In-Game Songs*: Those songs (in Hippel-CoSo format) can be extracted and played (be aware that the sound player is very rudimentary and poorly debugged).
- *Graphics*: Decoding for some graphics works, but the palettes aren't always obvious

The following don't do anything yet:
- *Intro/Outro*
- *Game scripts*
- *Maps*
- *Automaps*
- *Game Scripts*

Only tested on a late English version of the game.

## Decoding status and documentation
- All container formats can be decoded.
- Please check the [docs/FORMATS.org](WIP format descriptions)


| File         | Decoded                           |
|--------------|-----------------------------------|
| AMBERDEV.UDO | only very partially               |
| Amberload    |                                   |
| AUTOMAP.AMB  |                                   |
| BACKGRND.AMB | only partially, palettes missing  |
| CHARDATA.AMB |                                   |
| CHESTDAT.AMB |                                   |
| CODETXT.AMB  | yes                               |
| COL_PALL.AMB | mostly, missing exact RGB mapping |
| COM_BACK.AMB | Missing palettes                  |
| EXTRO.UDO    |                                   |
| F_T_ANIM.ICN |                                   |
| ICON_DAT.AMB | partially                         |
| INTRO_P.UDO  |                                   |
| INTRO.UDO    |                                   |
| LABBLOCK.AMB |                                   |
| LAB_DATA.AMB |                                   |
| MAP_DATA.AMB |                                   |
| MAPTEXT.AMB  | yes                               |
| MON_DATA.AMB |                                   |
| MON_GFX.AMB  |                                   |
| PARTYDAT.SAV |                                   |
| PICS80.AMB   | mostly, missing palette bindings  |
| PUZZLE.ICN   |                                   |
| PUZZLE.TXT   |                                   |
| SAMPLEDA.IMG | yes                               |
| TACTIC.ICN   |                                   |
| TH_LOGO.UDO  |                                   |
| WARESDAT.AMB |                                   |

## Links
- Ambermoon resources: https://github.com/Pyrdacor/Ambermoon

## Acknowledgements
This work is based by documentation collected from the following sources:
- [https://www.pyrdacor.net](Pyrdacor)'s collection of AmberMoon documentation: [https://github.com/Pyrdacor/Ambermoon]
- Heikki Orsila's [https://zakalwe.fi/uade/](UADE), specifically m68k assembly implementations of Jochen Hippel's sound formats
- Christian Corti's [https://github.com/photonstorm/Flod/blob/master/Flod%204.1/neoart/flod/hippel/JHPlayer.as](Flod 4.1 player for Jochen Hippel's 4-voice formats)
