# Pixel-art sprites

Game content (CC-BY-SA-4.0, like the rest of the game's content layer).

Pixel art at one density: 48 px for the 54-unit infiltrator
(`sprites::SPRITE_PX` = 48/54 art pixels per canvas unit), on one shared
38-colour palette, with a one-pixel off-black outline. Drawn at true size and
sampled nearest-neighbour; the floor camera runs at three screen pixels per
art pixel (`scene::PIXEL_SCALE`). Figures face right; the game flips them.

The walking, standing, crouching and jumping figures are MoikMellah's CC0 MV
platformer bases (see `../CREDITS.md`), dressed by layer onto the palette
(`dress_mv.py`). A Qwen-Image 2.1 edit redrew each character's stand and
walk row as the design (the guard's cap and build, the hacker's jacket);
that design was then carried onto every MV frame (`mv_transfer.py`): the
clothing recoloured to match it, and its head (and, on upright frames, its
whole upper body) stamped on at each frame's head position. Every frame's
motion is MV's own pixels, so nothing between frames changes except what
moves. The sneak is the walk with the knees bent and the body leaning
forward. The seated typing poses (animated by moving only the hands a pixel,
`type_loop.py`) and everything else: subjects drawn by Qwen-Image 2.1 (text-
to-image, with edits for the crouch and the open van), then either redrawn
as pixel art by SDXL 1.0 with the Pixel Art XL LoRA (people and most props)
or shrunk onto the palette by dominant colour (small items, the locker). The
fire exit's sign and window, and the kitchen and hall wall tiles in
`../backdrop/`, were placed by hand. The PNGs are embedded at build time
(`sprites::PaintedSprites`, `backdrop::BackdropArt`), so a re-cut sprite at
the same density drops in.