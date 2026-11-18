# Daggerfall Dungeon Maker

A CAD-style 3D viewer and editor for the dungeons and dungeon cells (RDB
blocks) of *The Elder Scrolls II: Daggerfall*, written in Rust. This is a project being continuisly 
implemented and prooved upon and has the following capabilities so far.

1. Import and view all Daggerfall dungeons and cells.
2. Create your own project and save it
3. Create your own Dungeons
4. Create your own cells
5. Replace existing dungeons in Daggerfall
6. Add new dungeons in Daggerfall
7. Export as a dfmod (requires Unity to be installed) or loose files to be used in DFU.
8. Use the ingame music system or provide your own MP3 or OGG to play when entering the dungeon (only as a dfmod)

What I plan to add to this in the near future

1. Custom models and textures (most likely only for dfmod)
2. Texture replacement on models
3. City, town, village editor with cell editor similar to dungeons
4. Interior editor for houses
5. Custom or replace cities/towns/villages

## Game data

No game data is included in or generated into this repository. On first launch
the program asks for your Daggerfall `ARENA2` folder (for example
`.../steamapps/common/The Elder Scrolls Daggerfall/DF/DAGGER/ARENA2`) and reads
`ARCH3D.BSA`, `BLOCKS.BSA`, `MAPS.BSA` (optional, for dungeons and the world
map), `POLITIC.PAK` (optional, for the regions on the world map), `ART_PAL.COL`
and the `TEXTURE.###` files from it at run time. File names may be upper or
lower case. Common Steam and GOG install locations are detected automatically.

Only the folder path is remembered between runs, in the app's settings
(`%APPDATA%\Daggerfall Dungeon Maker` on Windows,
`~/.local/share/daggerfalldungeonmaker` on Linux); change it with
**File > Set ARENA2 folder…**.

## Building

The program runs on Windows and Linux.

### Windows

```powershell
./build.ps1
```

This runs `cargo build --release` and produces
`target\release\daggerfall-dungeon-maker.exe`. With the GNU Rust toolchain, MinGW-w64
must be installed (`scoop install mingw`); see `build.ps1` for details.

### Linux

Install Rust from <https://rustup.rs> and the system libraries used for the
window, file dialogs and graphics. On Debian or Ubuntu:

```sh
sudo apt install build-essential pkg-config libgtk-3-dev libxkbcommon-dev   libwayland-dev libx11-dev mesa-vulkan-drivers
```

Then build:

```sh
./build.sh
```

This produces `target/release/daggerfall-dungeon-maker`. A Vulkan or OpenGL
capable graphics driver is needed to run it. Both X11 and Wayland desktops work.

### Command line

Optional command line arguments:

```text
daggerfall-dungeon-maker [project.ddm] [--arena2 <folder>] [--cell <name.RDB or name.RMB>]
                         [--dungeon <name>] [--settlement <name>]
```

`--cell` opens a dungeon cell (`.RDB`) or an outdoor cell (`.RMB`);
`--settlement` opens the largest location of that name.

## Using the viewer

* The **project panel** (left) lists everything that can be opened. The
  toggle at its top chooses **Dungeons** or **Settlements** (see
  [Settlements](#settlements) below). For dungeons, it lists one kind at a
  time: **Dungeons** or **Cells**, chosen below the toggle. The **Daggerfall**
  switch beside them shows or hides the game's own (dungeons grouped by
  region); below them are the project's own, in their folders. Opening
  something of the other kind switches to its section. Click a dungeon or
  double-click a cell to open it; the search box filters the list shown. The
  section and toggle are remembered between runs.
* The **+** beside the search box creates an empty dungeon (in Dungeons) or an
  empty cell (in Cells) named with what is typed, and opens it; Enter in the
  search box does the same. It is greyed out until a name is typed, and while
  the name is taken by anything in the project or by one of Daggerfall's own
  dungeons or cells of that kind (hover it to see why). A new dungeon has no
  cells or location yet; a new cell has no objects.
* **File > Open cell…** (Ctrl+O) lists all 187 RDB blocks, filterable by type:
  `N` interior, `W` interior with water, `M` interior variant, `S` special
  (quest) and `B` border caps.
* **File > Open dungeon…** (Ctrl+Shift+O) lists every pre-built dungeon in
  `MAPS.BSA`, searchable and filterable by region. The dungeon is shown with
  all of its cells in place. Hovering a cell highlights the whole cell and its
  block outline; double-clicking it opens that cell in a separate window with
  the full cell editor (closing the previously opened cell window). The window
  starts with the dungeon selected under **Used in**, so the dungeon's other
  cells show faded around the cell. Changes
  made in that window appear in the dungeon view. As in the game, the exit door
  (model 70300) is only kept in the starting cell, labelled **Start**; opening
  a cell on its own with **Open cell…** still shows all of its doors.
* Clicking a cell selects it; Shift+click adds cells to the selection (or takes
  one out), and Shift+dragging a box selects every cell whose middle is inside
  it. Shift+click works in the cell list too; Escape or clicking empty space
  clears the selection. With two or more cells selected, their right-click menu
  has **Merge N selected cells**: it makes one new cell of your own from them,
  named after the dungeon, and puts it in the dungeon in their place. Every
  object keeps its place in the dungeon: the new cell takes the start cell's
  grid spot (or, without the start, the one with the lowest grid column, then row), and the
  other cells' objects reach out past its square. A merged cell has one start
  marker (and so one water level, the status bar says if they differed), and
  only the start cell's exit doors stay, as in the game. The cells merged are
  not changed, and your own cells among them stay in the project. Cell windows
  that were open close. A cell can use at most 750 different models, so a merge
  that would need more is refused.
* With a skybox chosen (see **Skybox** below), the dungeon view draws it behind
  the dungeon, mapped as Daggerfall Unity maps it, so it faces the same way as
  in the game. **View > Skybox** turns it off and on. An image that cannot be
  read is named, with the reason, in the top left of the view.
* **Dungeon textures**: Daggerfall Unity gives every dungeon its own wall and
  floor textures, swapping Daggerfall's standard dungeon archives (119, 120,
  122, 123, 124 and 168) for a table of six picked from the dungeon's location
  id or map id and the climate of its map pixel; building doors (archive 74)
  take their climate's variant. The dungeon view, and a cell view with a
  dungeon selected under **Used in**, draw the dungeon's models with the
  textures the game gives it, as the game draws them (with the standard
  textures' texture coordinates; the exit door and tapestries without an action
  keep the standard textures). How the game picks the table is its
  `RandomDungeonTextures` setting, which the editor reads from Daggerfall
  Unity's `settings.ini`: classic (0, the default), by climate (1 and 2) or
  random (3 and 4; main story dungeons keep their classic tables in 0, 1 and
  3). **View > Dungeon textures** chooses **As in Daggerfall Unity** (your
  setting), **Standard** (Daggerfall's standard textures everywhere) or any
  one setting. The random settings use Unity's random numbers, which the
  editor imitates but has not checked against the game. A project dungeon's
  textures are those it will have once exported, so it shows the standard ones
  until it is placed on the world map; the properties panel's **Textures** row
  lists the table.
* **Doors the game switches off**: after laying a dungeon out, Daggerfall Unity
  switches off every action door within 1.4 m of another door, keeping the
  first in the dungeon's order (cell by cell, and in each cell by object
  list), and of the start cell's exit door. Two cells with a door each on the
  edge they share end up with one, as many of Daggerfall's own dungeons do.
  The dungeon view and the cell view tint those doors red
  (**View > Switched-off doors** turns it off), the dungeon's properties say
  how many there are, a switched-off door's properties say so, and the export
  window lists them. (The game places the exit door's point as if the start
  cell were at the dungeon's corner, ignoring where it is; the editor does the
  same so it finds the game's doors.)
* The dungeon view's properties panel (right) shows the dungeon's name, type,
  location (including its region) and cells. Hovering a cell in its list highlights it, and
  double-clicking opens it. The name and type of a project dungeon can be
  changed there; the type decides which enemies its random enemy markers become.
* Under **Location** is where the dungeon is in the world. For Daggerfall's
  dungeons it shows their map position and location type; **Show map** shows
  them on the world map and **View…** their entrance. A project dungeon is meant to be added to the
  game as a new location, so there you choose:
  * **Map position**: its spot on the world map, in map pixels (x 0–999 from
    west to east, y 0–499 from north to south). **Map…** opens the world map in
    a window of its own: each region in its own colour, the sea in blue, every
    location as a dark dot and every dungeon as a red one. Scroll to zoom, drag
    to pan, and click a spot to choose it: an orange marker shows the choice,
    and the bar along the bottom gives its position and region. Nothing changes
    until **Select Location** (bottom right) is pressed, which places the
    dungeon there and closes the map; closing the map instead keeps the old
    spot. Pointing at a spot shows its region and what is already there. Every project dungeon with a
    spot is marked with a diamond and its name: the one being placed in yellow,
    the others in cyan. The x and y fields nudge it afterwards, and **Remove**
    takes it off the map again. The search field above the map finds a
    location by name, suggesting Daggerfall's locations and your dungeons as you
    type; choosing one, or **Find** (or Enter) for the best match, zooms in on it
    and rings it in white. A spot that already has a location (Daggerfall's or
    another of your dungeons) cannot be taken: clicking it says what is there,
    and the x and y fields will not move the dungeon onto it. The exception is
    one of Daggerfall's dungeons: clicking it chooses to **replace** it (the
    bar says "Chosen: replace …", the marker "(replaces …)"), and **Select
    Location** puts the dungeon in its place. A dungeon can
    only be placed once it has an
    entrance (to get in) and an exit (an exit door in its start cell, to get
    out); until then the map can be looked at but not clicked, and it says
    what is missing.
  * **Replaces**: shown when the dungeon replaces one of Daggerfall's, naming
    it, for example "Privateer's Hold (Daggerfall)". In the game the dungeon
    takes that dungeon's place: its spot, and its name, which quests and the
    travel map know it by (the x and y fields cannot move it). **Stop
    replacing** (or **Remove**) takes it off the map so it can be placed on a
    free spot instead.
  * **Region**: the region of its spot, set by the map position (or that of
    the dungeon it replaces).
  * **Location type**: its icon on the travel map (large, medium or small
    dungeon, or graveyard).
  * **Discovered**: whether the travel map shows it from the start of the game.
  * **Entrance**: what stands above ground around the way down (ruins, a
    castle, a graveyard…), named by its kind and block, for example
    "Ruins · RUINAA08". The button opens a window of its own listing every
    entrance Daggerfall's dungeons have: their 4,232 dungeons share 136
    entrances, each a grid of RMB blocks. The list can be searched by kind,
    block or dungeon name. The selected entrance is drawn in 3D on a patch of
    ground, with its dungeon door highlighted (right-drag to turn, scroll to
    zoom); the dungeons that have it are listed above, and **Use this
    entrance** gives it to the dungeon. An entrance without a dungeon door
    model is flagged, since the player may not be able to go in.
  * **Music**: the song played inside. **Chosen by the game** leaves it to the
    game (for a dungeon that replaces one of Daggerfall's, that dungeon's song,
    shown in the list); otherwise one of Daggerfall Unity's 15 dungeon songs,
    "Dungeon 1 (DUNGEON)" to "Dungeon 15 (28)". Daggerfall Unity picks a
    dungeon's song from a value in its location data and the region, so the
    export writes a value that picks the song chosen. With FM music the game
    plays the FM version of the same song; music mods may play something else.
    For Daggerfall's own dungeons the row shows the song they play. **▶ Play**
    plays the song (read from `MIDI.BSA`) to preview it and turns into **■ Stop**
    while it plays; choosing another song stops it. On Windows and macOS it plays through the system's MIDI
    synthesizer; on Linux it needs TiMidity, or FluidSynth with a General MIDI
    sound font, installed.
  * **Custom music**: ticking **Play my own music file** gives the dungeon a
    music file of your own instead (OGG or MP3), chosen in the **Music file**
    box below it (**Browse…** opens a file dialog). The game songs above are
    greyed out then. Only a `.dfmod` can play it: the export copies the file
    into the mod's `Music` folder and adds a C# script
    (`<mod>DungeonMusic.cs.txt`), which Daggerfall Unity compiles when it loads
    the mod. Inside the dungeon (recognised by its region and name) the script
    stops the game choosing songs, fades the song playing out over half a second
    and stops it for good, then plays the file on a loop at the game's music
    volume; leaving the dungeon gives the game's music back. Dynamic Music, which
    plays music with a player of its own, is faded out and paused the same way
    (other music mods that do so may still play over it). With the exported
    files used loose there is no script, so the dungeon plays the song chosen
    under **Music** instead. A file that is missing or not OGG/MP3 is listed as a
    problem and stops the export.
  * **Skybox**: ticking **Show a sky** shows a sky of your own behind the
    dungeon instead of Daggerfall's black, seen through any openings to the
    outside (a broken ceiling, a chasm, an open courtyard). Choose the image in
    the **Sky image** box below it: a PNG or JPG panorama of the whole sky, twice
    as wide as it is high (for example 4096 × 2048). Only a `.dfmod` can show it:
    the export copies the image into the mod's `Sky` folder and adds a C# script
    (`<mod>DungeonSky.cs.txt`), and the Unity build turns each image into a
    panoramic skybox material. Inside the dungeon the script makes the camera
    draw that skybox, and keeps the game's black dungeon fog (which would
    otherwise cover the sky completely) off it; leaving gives the game's own
    settings back. With the
    exported files used loose the dungeon stays black. An image that is missing
    or not PNG/JPG is listed as a problem and stops the export.

  Warnings below list what is still missing: a spot, a spot that is not in the
  sea or already taken (by a Daggerfall location or another project dungeon,
  since two locations cannot share a map pixel), an entrance and an exit. A copy of a
  Daggerfall dungeon starts with the original's location type and entrance but
  no spot, since the original's is taken; a duplicated dungeon loses its spot
  for the same reason.
* Right-clicking a cell in the dungeon view or in the Cells list offers
  **Make Start**, which makes it the dungeon's start cell (where the player
  enters) and stops the previous one from being it. Only the start cell keeps
  its exit doors, so Daggerfall cells gain or lose them to match. A cell
  without an exit door (model 70300), from which the player could not leave,
  or without a start marker, where the player arrives, cannot be the start:
  the option is disabled and the menu says "No exit exists for this cell"
  and/or "No start marker exists for this cell". In a dungeon from Daggerfall
  it offers to make a copy first.
* Dragging a cell with the left mouse button moves it around the dungeon: it
  follows the pointer from grid square to grid square, highlighted, with "Move …
  to x, z" beside it. Let go on a free square and it stays there; on a square
  another cell already has ("A cell is already here") it goes back. In a dungeon
  from Daggerfall it goes back and offers to make a copy first.
* Pressing **Delete** while hovering a cell (in the dungeon view, or its row in
  the Cells list), or choosing **Delete** in its right-click menu (in either
  place), takes it out of the dungeon. A project cell it used stays in
  the project; a cell window open on it closes. Taking out the start cell leaves
  the dungeon without one until another is made the start. In a dungeon from
  Daggerfall it offers to make a copy first.
* Dragging a cell from the project panel's **Cells** list (yours, or
  Daggerfall's with the **Daggerfall** switch on) into the dungeon view adds it
  to the dungeon. While dragging, a faded copy of the cell shows on the grid
  square under the pointer, outlined, with "Drop to add … here"; dropping it
  there adds it. A square that already has a cell shows "A cell is already here"
  and takes nothing. A project cell dropped in is used as it is (not copied), so
  changing it changes it in every dungeon that uses it. In a dungeon from
  Daggerfall it offers to make a copy first.
* The outliner (left) lists every object: first the **object lists** you made
  (🗀 folders, see below), then the rest by the **quarter** of the cell they
  are in. Daggerfall lists a cell's objects that way, in squares of 25.6 m
  (south-west, south-east, north-west, north-east, and past the north or east
  edge for objects standing on or beyond it), and the game goes through them
  in that order; every one of Daggerfall's 187 cells keeps to it (five number
  their lists on a grid twice as wide, which the editor reads the same way).
  Objects join the quarter they are placed, pasted or moved into.
* **Object lists** are folders of your own in the outliner, to keep a cell
  organised; the game never sees them. Right-click an object, type a name if
  you like and choose **Create Object List**: every object whose middle is
  inside the object's box (as **Select All Within** finds them, below), the
  object included, goes into the new list; an object already in a list moves
  to the new one. Right-clicking a list in the outliner renames it, selects
  its objects (**Select All**) or removes it (**Delete List**, which leaves
  the objects in the cell). An object's menu can also take it out of its
  list. Lists are saved in the project with the cell; copies stay in their
  list, and copies pasted into another cell join its list of the same name.
  The properties panel (right) edits name, visibility, position and rotation,
  and under **Game settings** lists every setting Daggerfall stores with the
  object, set or not: for models the trigger, starting lock, sound, action
  (type, axis, duration, magnitude) and the object the action triggers next
  (with **Go to**, which selects and frames it); for flats their flags, action, magnitude, sound and link;
  for lights two values of unknown meaning. All of them can be changed in
  project cells. Action links are kept by object id, so they survive edits and
  are saved in project files. In the viewport each link is a red line, drawn
  over walls, from the object to the one it triggers, with an arrowhead at the
  target (**View > Action links** turns them off). Links stay within one cell.
* **Scale**: ticking **Scale** in a model's or flat's properties lets you
  change its size: X, Y and Z for a model (along its own axes), width and
  height for a flat, from 0.05× to 20×, or every axis at once with **All**.
  **Reset** sets it back to 1× and unticking **Scale** gives the object back its
  usual size. Daggerfall's cells cannot store a size, so only a `.dfmod` shows
  it in the game: there the export leaves scaled objects out of their cells and
  a script in the mod puts them back, at their size, when the dungeon is laid
  out. Loose files keep them at their usual size, and the export window warns
  about it. Objects the game gives behaviour of their own cannot be scaled:
  lights, markers, treasure, people (the game makes them NPCs that quests can
  use), doors, the exit door, objects with an action and objects linked to
  another's action (the checkbox says why). Scaled tapestries and banners have
  no collision, as in the game. A scaled torch or animal is silent (the game
  only gives its own copies their sound), and models that other mods replace
  are shown as Daggerfall's own when scaled. The other objects in the cell keep
  their positions, so saved games still find them. Checking for openings to
  empty space uses scaled models' usual size.
* **Preview action**, under Game settings, appears for objects whose action
  moves something: it plays the action in the viewport as Daggerfall Unity
  does, with every object down the chain moving at the same time (moves,
  slides, rotations, and doors that an action opens swinging open), holds the
  end for half a second and puts everything back. The cell is not changed.
  Like Daggerfall Unity, it spins wheels (such as model 61032) a full turn
  around their axle and turns coffin lids 90°, whatever axis their action
  stores, and reads a few single traps' unusual axis values as the game does.
* Several objects can be selected with Ctrl+click (in the viewport or the
  outliner; Ctrl+click a selected object to take it out; in the viewport
  Shift+click does the same). In the outliner, Shift+click selects every row
  from the one clicked last to the one clicked, as the outliner shows them
  (rows under a closed heading are skipped), and Ctrl+Shift+click adds them to
  the selection. Objects can also be selected by dragging a box
  over empty space in the viewport: the objects inside it light up, and
  releasing selects them (Shift adds them to the selection). The gizmo of
  several selected objects sits at their middle and moves them all together,
  as one undo step. With grid lock on, the last selected object lands on the
  grid and the others keep their places relative to it. Several objects can't
  be turned at once, unless they are prefabs.
* **Prefabs** are groups of objects saved to place again together. Select
  several objects (models, flats, markers, lights), right-click one of them (or
  empty space) and choose **Create Prefab**, after typing a name if you like.
  The prefab is added to the **▣ Prefabs** tab of the asset palette, and the
  objects become a placed copy of it. Clicking a prefab in the palette arms all
  of its objects: the ghost follows the mouse with the prefab's base (the
  middle of the bottom of the box around its objects) on the surface, Q, E or
  R turn it, and a click places it. Prefabs are saved with your settings, like
  favorites, so every project can use them; right-clicking one in the palette
  renames it or deletes it from the list (**Delete prefab**), which leaves the
  copies already placed as they are.
* A placed prefab stays together: hovering any of its objects lights them all
  up inside a green box, and clicking one selects the whole prefab, which then
  moves (the arrows), turns (R, Q, E, around its middle), copies, duplicates and
  deletes as one. Its objects are marked ▣ in the outliner, and its own
  properties can't be changed one by one: the properties panel shows the
  prefab instead. Right-click it and choose **Break Prefab** (or use the button
  in the properties panel) to turn it back into separate objects. Placed
  prefabs are saved in the project with the cell; the game sees their objects
  as any others. Copies, pasted copies and the cells of a merge each keep
  their prefabs apart.
* **Copy and paste**: Ctrl+C (or **Edit > Copy**) copies the selected objects,
  one or many, and Ctrl+V (**Edit > Paste**) pastes them, selected, as one undo
  step. Pasted into the same cell they land 1 m along from the originals; into
  another cell (in the main window or a cell window), where they were. Each
  further paste lands another 1 m along. The copies get new ids; action links
  between copied objects follow the copies, and links to other objects are only
  kept in the cell they were copied from. Ctrl+D duplicates the selected
  objects and Delete deletes them, however many are selected.
* A single selected object's gizmo also has three **rotation rings** (red,
  green and blue, around X, Y and Z). Dragging a ring turns the object around
  that axis by the angle the pointer sweeps round the gizmo, in 15° steps with
  grid lock on (Ctrl switches it for that drag); the status bar shows the angle.
* A selected object whose action moves it shows its **trajectory**: a purple
  line along the path its middle takes, from a ring where it is to a dot where
  the action leaves it (a straight line for moves and slides, an arc for
  turns). Every object its action sets moving shows its trajectory too, down
  the chain to its end. It is worked out every frame, so it follows any change to the
  action's type, axis or magnitude, or to the object's position.
* Right-clicking an object (in the viewport or the outliner) opens its menu;
  right-clicking one of several selected objects opens it for all of them.
  **Rename** edits the object's name (or a prefab's) as you type, as one undo
  step; clearing a model's name lists it by its model's name again.
  **Duplicate** and **Delete** work as Ctrl+D and Del. **Select All Within**
  selects every object whose middle is inside the object's box (the one drawn
  around it when selected, turned with it), the object included; for a prefab
  or several selected objects it uses the box around them all. It counts only
  the objects the view shows, and prefabs it reaches into come whole.
  **Create Object List** puts those objects in a new object list (see above).
  **Tie action to…** draws a red line from the object to the pointer; the
  object under the pointer is highlighted, and clicking it makes the action
  trigger it (Escape, right-click or clicking empty space cancels). A model
  without an action gets an empty one, to be given a type under Game
  settings. On an object that other objects' actions trigger, **Remove
  actions** removes those actions, as one undo step.
* With nothing selected, the properties panel shows the **cell**: name,
  category, source, size, how many objects and object lists it has, and its water level
  and castle flag (which live on the cell's start marker). A project cell's
  category is a drop-down list (`N` interior, `W` interior with water, `M`
  interior variant, `S` special, `B` border cap); a copy starts with the
  category of the cell it was copied from, a new cell as an interior, and a
  merged cell with that of the cell whose place it takes. A cell with water
  shows its water surface as a see-through blue plane across its square, in
  the cell editor and the dungeon view.
* Every cell covers a 51.2 × 51.2 m square of the dungeon grid. Objects that
  reach outside it (by their model's box, or their position) can overlap the
  next cell in a dungeon, so they are marked ⚠ in the outliner, warned about
  in their properties and in the status bar after moving them, and counted in
  the cell's properties, with **Select them** to select them all. (Daggerfall's
  own cells have 210 such objects in 68 cells, mostly by under half a metre.)
* Selecting a marker adds a **Marker** section naming its kind and what the game
  reads from it: water level and castle flag on start markers, slot and spawn
  distance on random enemy markers, spawn distance (as well as the enemy) on
  fixed enemy markers. Enter, quest, quest item, treasure and teleport markers
  carry no data; the game uses only their position. A teleport marker is where
  the player lands when an object whose action is Teleport, tied to the marker,
  is triggered.
* Resting the pointer on any property or button for half a second explains
  what it does.
* **Used in**, at the bottom of the properties panel, lists every dungeon that
  uses the cell (for a cell copy, the project dungeons using that copy).
  Clicking one shows that dungeon's other cells, faded and outlined, around
  the cell being edited (click it again to hide them); double-clicking opens it
  in the dungeon view. Hovering a faded cell outlines it and names it;
  double-clicking it opens that cell instead, with the same dungeon still
  selected and shown around it (in a cell window of the open dungeon, the cell
  opens in a cell window as if double-clicked in the dungeon view).
* **View** toggles back-face hiding (to look into cells from outside), the grid,
  the block outline, the asset palette, models, flats, editor markers, lights,
  action links and the red tint of doors the game switches off, and chooses
  the dungeon textures (see **Dungeon textures** above).

### Settlements

With **Settlements** chosen at the top of the project panel, it lists the
world above ground, from Daggerfall's own data. These open read-only; making
settlements and outdoor cells of your own is not supported yet.

* **Settlements** lists every location of the world (all 15 251: cities,
  hamlets, villages, taverns, temples, homes, farms, covens and the dungeons'
  entrances), by region and then by type. The search box finds them by name,
  region or type. Click one to open it.
* **Cells** lists the outdoor cells (RMB blocks) locations are built from, by
  kind (taverns, residences, city walls, guild halls and so on). They can be
  given names of your own, as dungeon cells can. Double-click one to open it.

A settlement is shown with all of its cells on their grid: their buildings,
scenery, flats and trees, standing on their ground, drawn as Daggerfall Unity
(1.1.1) draws them in the location's climate:

* The buildings' and other models' textures are swapped for the climate's
  variants (`ClimateSwaps.ApplyClimate`): a desert town's walls and roofs are
  the desert's, a swamp's the swamp's.
* The ground is drawn tile by tile with the climate's ground textures, each tile
  turned as the cell says (a quarter turn anticlockwise, a half turn, or both,
  as the game's terrain shader turns them).
* The trees, rocks and plants are the climate's.
* In winter (the **Season** list), the ground, the trees and the buildings take
  their snowy textures where they have them; deserts have no winter.

Tiles marked past the end of the ground textures are the "blend space" Daggerfall
Unity leaves around walled cities (and a few other cells), where the world's own
terrain shows: grass, with patches of dirt and stone that depend on the spot.
They are drawn as the climate's plain ground. Hovering a cell highlights it and
its outline; double-clicking it opens that cell on its own. **View > Frame
settlement** (Home) shows it all again.

Its properties (right) show its name, region, type, place on the world map,
whether the travel map shows it from the start, its climate, its size in cells,
the dungeon under it (click to open the dungeon), its buildings by type, and its
cells (hover to highlight one, double-click to open it). The **Climate** list
draws it in another climate, to see how it would look there, and the **Season**
list in summer or winter; these change the view only.

An outdoor cell opened on its own shows its kind, its objects and buildings,
your name for it, and **Used in**: every location built with it (click one to
open it). It is drawn in the woodlands climate, in summer, unless others are
chosen.

The entrance window of a dungeon draws each entrance the same way, in the
climate of the dungeon's spot on the world map.

Dungeons are drawn with the wall and floor textures Daggerfall Unity gives them
where they are (see **View > Dungeon textures**): picked from the dungeon's ids
and the climate of its spot, with the climate's building doors.

### Projects

Cells and dungeons from Daggerfall open read-only. Trying to change one (moving,
placing, deleting or editing an object) shows *You cannot edit cells from
Daggerfall* (or *dungeons*) with **Create a copy** and **Cancel**. A copy is
named after the original with `-Copy` added, for example `N0000004-Copy` or
`Privateer's Hold-Copy`, and opens in place so the change can be made again.
Right-clicking a Daggerfall entry also offers **Create a copy**.

The cells in a dungeon are Daggerfall's own cells too, so they stay read-only
even in a copied dungeon. Changing a cell in a Daggerfall dungeon copies both
the dungeon and that cell (*You cannot edit dungeons from Daggerfall*);
changing a Daggerfall cell in a copied dungeon copies just the cell for that
dungeon. The cell copy is an ordinary project cell the dungeon now uses, and
each such cell belongs to one dungeon: duplicating the dungeon duplicates its
cells too, and deleting the cell puts the Daggerfall cell back.

Copies belong to the project and are listed loose at the top level of the
project panel. Right-click them to open, rename, duplicate, move or delete them,
and use **New folder** (or right-click a folder) to organise them into folders,
which can be nested and renamed.

Entries can also be dragged: onto a folder to move them into it, onto another
entry to place them just before it, or onto the empty space below the list to
move them to the top level. Dragging a Daggerfall cell or dungeon into the
project adds a copy where it is dropped. Cells can also be dragged into an open dungeon (see
above).

**File > Save project** (Ctrl+S) and **Save project as…** (Ctrl+Shift+S) write
a `.ddm` project file; **Open project…** and **New project** switch projects,
asking first if there are unsaved changes, as closing the program does. The last
project is reopened on the next run, and a `.ddm` file can be passed on the
command line. Project files are JSON. A dungeon refers to the project cells it
uses by id; its other cells are read from the game data when it is opened. A
dungeon's location (map position, location type, discovered and entrance) is
saved with it.

### Exporting to Daggerfall Unity

**File > Export to Daggerfall Unity…** turns the project into a
[Daggerfall Unity](https://www.dfworkshop.net/) mod that adds the project
dungeons you choose to the game as new locations. The window has:

* **Mod**: the mod's title, version, author, contact and description, shown in
  Daggerfall Unity's mod list. They are saved with the project.
* **Dungeons**: every project dungeon with a tick box; the ticked ones go into
  the mod. Only dungeons placed on the world map can be ticked (placed ones start
  ticked). A ticked dungeon also needs an entrance, a dungeon type, one start cell
  with an exit door, a free spot in a region, and a name no other location of its
  region has. Anything missing is listed under it, and the export waits until it
  is fixed or the dungeon is unticked. **Check again** re-checks after changes.
  Notes in yellow under a dungeon say what the game does differently from the
  editor without stopping the export: the doors it switches off, and (for a
  dungeon replacing Castle Daggerfall) whether its foyer doors fix still works.
* **Output**: the folder to export into. A folder named after the mod is made
  there, holding:
  * `WorldData/location-<region>-<index>.json` for each dungeon that replaces
    one of Daggerfall's: the file Daggerfall Unity reads instead of that
    location. It keeps the location's index, map id, location ids, position,
    climate and name, so quests and saved games still find it, with the
    project dungeon's cells, entrance, type and settings;
  * `WorldData/locationnew-<dungeon>-<region>.json` for each other dungeon: its
    location, entrance (the above-ground blocks of the Daggerfall dungeon chosen
    as entrance), climate and grid of cells;
  * `WorldData/<BLOCK>.RDB.json` for each project cell the dungeons use, under a
    new block name: the letter of the cell's category and a number from
    9000000 up, for example `N9000004.RDB`. Daggerfall's own cells are used by
    their own names and are not exported;
  * `Models/<id>_model.json` and `Models/<id>_texture_<n>.png` for each of your
    imported models the cells use (in an export for a `.dfmod` only);
  * `<mod>.dfmod.json`, the mod's manifest, and a `README.txt` listing what is in it.

  The `WorldData` files work as they are when copied into Daggerfall Unity's
  `DaggerfallUnity_Data/StreamingAssets/WorldData` folder, without a mod.
* **Files only** or **Build a .dfmod with Unity**: whether to also pack the
  files into a `.dfmod` (a Unity asset bundle). With **Files only**, if a ticked
  dungeon has custom music, the window warns that custom music is not supported
  in loose files and those dungeons fall back to the game's own music; if one has
  a skybox, it warns that skyboxes do not work in loose files and those dungeons
  stay black; if one has scaled objects, it warns that scaled objects do not
  export in loose files, only in a `.dfmod`, and they keep their usual size; if
  one uses custom models, it warns that loose files cannot carry them and they
  will be missing in the game. The export still goes ahead. An export for a `.dfmod` leaves scaled objects out of
  the `WorldData` cells (the mod's script adds them), so for loose files export
  with **Files only**. Tick the platforms to build for;
  each gets its own `.dfmod` in a `Windows`, `Linux` or `macOS` folder next to the
  files, and **Copy to Mods folder** also puts this computer's build into
  Daggerfall Unity's `Mods` folder.
* **With Unity**, the `.dfmod` is built by running the Unity editor in the
  background, as Daggerfall Unity's Mod Builder does. It needs Unity 2019.4 (Daggerfall Unity uses 2019.4.40f1),
  installed with Unity Hub and signed in to a licence. **Find** looks for
  editors installed by Unity Hub. Each platform needs its build support installed
  in the editor. The first build makes a small Unity project to build in (in the
  local application data folder) and takes a few minutes; later builds take about
  a minute. The build makes each custom model the cells use a prefab named after
  its id (its mesh, a material for each of its images, and a collider), which
  Daggerfall Unity puts wherever a cell uses that model, as it does for any
  mod's models; the cells also tag the model `COL`, which gives it a collider.

The output folder, Unity editor and platforms are remembered between runs.
Block names, location ids and the mod's GUID stay the same from one export to
the next, so saved games keep working with an updated mod. A new dungeon's
location id is picked from its map pixel, in the project's order, the same
whichever dungeons are exported with it (the dungeon textures depend on it).

Castle Daggerfall has a fix in Daggerfall Unity: a player who teleports into
the castle can open the two magically held doors in its foyer (model 55004 in
its start cell, `S0000160.RDB`), so they are not locked in the throne room.
The game finds the doors by their ids, which are the cell's position in
`BLOCKS.BSA` plus the doors' offsets in it. A project dungeon replacing Castle
Daggerfall that uses its own copy of that cell, with the doors where they
were, exports the cell with the original cell's position and the doors'
original offsets, so the fix still works. If the doors were moved or removed,
or the copy has more objects than fit under the original cell's ids, the
export window says the fix will not work.

### Enemy markers

Random enemy markers (`TEXTURE.199` record 15) do not name an enemy: the game
picks one from a table for the dungeon's type (crypt, orc stronghold, vampire
haunt, …) and the player's level. Markers below the cell's water level use a
separate underwater table (mostly slaughterfish and dreugh) in every dungeon;
the water level comes from the cell's start marker, as in the game. Every
dungeon's type is read from `MAPS.BSA` and shown in its title and tooltip.
Selecting a random enemy marker lists the enemies it can become, with their
chances for a player level you choose (type it, drag it or scroll over it);
hovering one shows the likeliest three. Above water this needs the cell to be
opened from a dungeon. The chances follow the original game's dungeon pick,
which Daggerfall Unity uses by default and which gives the same enemies each
visit for a given dungeon and player level. The tables are built into the
original executable rather than the data files, so they are taken from
Daggerfall Unity (`src/encounters.rs`).

Fixed enemy markers (record 16) store their enemy in the cell itself. Selecting
one shows what it spawns, whether it is hostile or passive (attacks only when
provoked) and, for human classes, its gender; hovering one shows the enemy too.
The same enemy appears in every dungeon that uses the cell. In a project cell
all three can be changed under **Spawns**, and **Nothing** switches the marker
off without deleting it. Fixed enemy markers placed from the asset palette start
as a rat (enemy 0).

### Customising a cell

The **asset palette** along the bottom lists everything that can be placed:

| Tab | Contents |
| --- | --- |
| ★ Favorites | The assets you marked as favorites, of every kind, in the order marked |
| ▣ Prefabs | Your prefabs: groups of objects to place together (see above) |
| Dungeon Models | Every model used in Daggerfall's dungeon cells, most widely used first |
| Exterior Models | Every model Daggerfall uses outdoors (buildings seen from outside, walls, scenery, dungeon entrances), from its RMB blocks, most widely used first |
| All Models | Every model in `ARCH3D.BSA` |
| Flats | Sprites used in cells: torches, furniture, people, treasure |
| Markers | Editor markers: start/enter points, enemy spawns, quest and treasure markers |
| Lights | A point light |
| Custom Models | The models you imported with **Import > Import Model…** (see below) |

**Import > Import Model…** imports a 3D model of your own, as Blender and most
3D programs export it:

* a **Wavefront OBJ** file, with its MTL materials and their textures (PNG, JPG,
  BMP or TGA). An OBJ file holds no image itself: it names its MTL file, which
  names each material's texture. Both are looked for where the files say, then
  by their names (whatever their case) beside the model, in the folder the name
  gives, or in a `textures` (or `tex`, `images`, `maps`, `materials`) folder
  beside it; options written before a texture's name in the MTL file are
  skipped;
* a **glTF** file: a `.gltf` with its `.bin` buffers and images next to it, or
  a single `.glb`. The meshes are placed as the file's scene places them (each
  node's position, turn and scale); each material is drawn with its base colour
  texture (tinted by its base colour, and moved by `KHR_texture_transform`) or
  in its colour. Files that need Draco mesh compression or other extensions
  the editor cannot read are refused with their names: export them again
  without those;
* a **Blender** file (`.blend`). Only Blender reads its own files reliably, so,
  as Unity does, the editor runs Blender without a window to export the file
  as glTF, then imports that. The status bar shows the conversion going on; the
  editor stays usable meanwhile. The export takes what the file's view layer
  shows (hidden objects are left out), with modifiers applied, without
  cameras, lights or animation, and does not run scripts saved in the file.
  Blender (free from [blender.org](https://www.blender.org)) must be
  installed: the editor finds it where Blender's installer, the system's
  packages or Steam put it, or on the `PATH`; **Import > Locate Blender…**
  chooses another one (its tooltip says which is in use).

If a model names a material file or texture that cannot be found or read, it
is still imported, its parts drawn in their colour. If its MTL file was not
found, a message asks whether to find it: the file chosen (whatever its name or
folder) gives the model its materials, the OBJ file being read again with it
(textures are then looked for beside the MTL file first, then beside the
model), keeping the model's id and name. Otherwise, a message says which
textures are missing and offers to choose the texture image at once.

A model's texture can also be chosen (or replaced) at any time: right-click it
under **Custom Models** and choose **Change texture…** (for a model of several
parts, one part, or every part without a texture). It takes an image, laid on
the model as its texture coordinates say, or an MTL file, used as above (if the
OBJ file the model came from is gone, the MTL file's materials go to the parts
of the same names, or its one material to every part). Either is saved with the
model and taken into the game by a `.dfmod` export.

**Delete model…** in the same menu deletes an imported model, after asking:
its saved files are removed from the application's data folder (the file it was
imported from is not touched) and it leaves **Custom Models**. If the project's
cells or your prefabs use it, the question lists where, and those copies are
removed too (save the project to keep that). Its id is never given to another
model, so a project file that still places it never shows a different model.

One unit is a metre, with Y up. Like Unity's own OBJ and glTF importers, the
editor mirrors the model into the game's left-handed space, so it looks in the
game as it did in the program it was made with. A material without a texture
is drawn in its colour. The model gets an id of its own, from 8000000 up, is
named after its file (right-click its tile to rename it) and is listed under
**Custom Models**, to place, snap, turn and scale like Daggerfall's models.
Imported models are converted once and kept in the application's data folder
(`custom_models`), so every project can use them; a project using one needs it
imported on the computer that exports it. Only an export for a `.dfmod` takes
them into the game (see above).

The list is built from your own game data when the ARENA2 folder is loaded.
Any tile can be marked as a **favorite**: hover it and click the ☆ in its
top-right corner (or right-click it and choose **Add to favorites**). Favorites
keep a gold ★ on every tab and are listed together on the **★ Favorites** tab;
clicking the ★ again (or **Remove from favorites**) unmarks one. Favorites are
saved with your settings, like your names for cells and models.
Click a tile to arm it; a green ghost follows the mouse over the cell surfaces.
Left click places it (Shift+click to keep placing), Q and E (or R) turn it
90° anticlockwise and clockwise, Esc cancels.

A model being placed **snaps** to the models already in the cell where their
open faces meet, and the face it snaps to is outlined in green. Open faces are
worked out from each model: the parts of its upright sides that its own
polygons leave uncovered, such as a corridor's ends or a doorway in a room's
wall. Two openings facing each other join flush, with their floors level; a
narrower opening can slide along a wider one. Thin models such as doors fit
into an opening, standing on its floor, and into the doorways inside pieces
where Daggerfall's own cells put doors (learned from every cell when the game
data loads), turned the way those doors are. The closest match in size wins,
then the nearest. A face something is already joined to (another piece, or a
door standing in it) is taken and not snapped to again. Hold Alt to place
without snapping. **Grid lock** on the menu bar (or G) snaps objects to the grid while they are
moved or placed: the position along the dragged axis lands on a grid line,
which is drawn faintly at the chosen step (0.4, 0.8, 1.6 or 3.2 m, that is 16,
32, 64 or 128 game units). Placed objects appear in the outliner under the quarter of the cell they
are placed in, and can be moved, rotated, duplicated, deleted and undone like any other object.

An opening of a hall or room piece that joins nothing and leads out into
empty space gets a **yellow pane** over it, in the cell editor and in the
dungeon view, and the dungeon's properties warn **Not all rooms are closed**,
naming the cells. It is only a warning: saving and exporting work as before.
An opening counts as closed when another piece's opening is joined to it, or
when most looks out through it (from its middle and four points around it) meet
a wall of the piece's own just inside it or any other model within 60 m. A door
standing in an opening does not close it, since at the dungeon's edge it would
open onto nothing. Only pieces at least 3 m tall and wide are checked, and only
openings a person could walk through. Some pieces have openings that look open by
this test but that Daggerfall's own dungeons leave so too (the space under a
ramp, the sides of cave chunks); these are learned from every Daggerfall dungeon
the first time the game data is loaded (it takes about half a minute in the
background, during which the properties say "Checking that every room is
closed…") and saved with the settings, and are not warned about. A cell edited
on its own does not warn about openings on its edges, where it meets its
neighbours; opened from a dungeon, the cells around it are taken into account.

| Input | Action |
| --- | --- |
| Click an asset tile | Arm it for placement |
| Left click while placing | Place it (Shift to keep placing) |
| Q / E or R while placing | Turn 90° anticlockwise / clockwise |
| Alt while placing | Place without snapping to open faces |
| Esc while placing | Cancel |
| Left click | Select object |
| Shift + left click | Add to or take out of the selection |
| Left drag on empty space | Select the objects inside a box (Shift adds them) |
| Left drag on gizmo arrow | Move along that axis (Ctrl switches grid lock for that drag) |
| Left drag on gizmo ring | Turn the selected object around that axis (15° steps with grid lock; Ctrl switches) |
| G | Turn grid lock on or off |
| Right drag | Orbit |
| Middle drag / Shift + right drag | Pan |
| Mouse wheel | Zoom |
| W A S D | Move forward / left / back / right along the ground, with the pointer over the view (Shift for faster) |
| Space / Ctrl | Raise / lower the camera, with the pointer over the view (right mouse + E / Q too). Ctrl lowers only when held on its own |
| F / Home | Frame selection / frame cell |
| E or R / Q or Shift + R | Turn the selected object 90° clockwise / anticlockwise (not a group) |
| Ctrl + D / Delete | Duplicate / delete selection |
| Ctrl + Z / Ctrl + Y | Undo / redo |

Edits are kept in memory only; saving changes back to a file is not
implemented yet.

## Code layout

| Path | Purpose |
| --- | --- |
| `src/arena2/` | Readers for BSA archives, ARCH3D models, RDB and RMB blocks, textures, palettes and the world map |
| `src/scene.rs` | Editable object list built from an RDB block |
| `src/project.rs` | Project files (`.ddm`): the user's cells, dungeons and folders |
| `src/encounters.rs` | Dungeon types and the enemies random enemy markers can become |
| `src/music.rs`, `src/arena2/hmi.rs` | Dungeon music: how the game picks a song, reading Daggerfall's HMI songs, and playing them |
| `src/export/` | Exporting the project as a Daggerfall Unity mod: world data JSON, manifest, music, skybox and scaled-object scripts, and the .dfmod built with Unity |
| `src/catalog.rs` | Placeable assets, gathered from the game data at run time |
| `src/prefabs.rs` | The user's prefabs: saving them, and placing their objects |
| `src/custom_models.rs` | Importing the user's own OBJ and glTF models, and keeping them |
| `src/blender.rs` | Finding Blender, and having it convert `.blend` files to glTF for import |
| `src/connectors.rs` | Open faces and doorways of models, for snapping pieces together |
| `src/dungeon_textures.rs` | The wall and floor textures Daggerfall Unity gives each dungeon |
| `src/doors.rs` | The doors Daggerfall Unity switches off because they overlap another door |
| `src/camera.rs`, `src/picking.rs` | Orbit camera and ray picking |
| `src/render/` | wgpu renderer (models, billboards, helper lines) drawing into an egui texture |
| `src/app/` | egui application: main window, menus, dialogs and cell editor windows |
| `src/app/project_panel.rs` | The project panel: dungeons or cells, Daggerfall's and the project's; settlements and outdoor cells |
| `src/app/exterior_view.rs` | The read-only view of a settlement or outdoor cell, and its properties |
| `src/exterior.rs` | Outdoor cells as scenes: their objects, and their ground in a climate's tiles |
| `src/app/room_editor/` | The cell editor: outliner, properties, asset palette, viewport, undo history |
| `src/app/dungeon_view.rs` | Whole-dungeon view with cell highlighting, and the dungeon's properties |
| `src/app/world_map.rs` | The world map window for choosing a dungeon's spot |
| `src/app/entrance_picker.rs` | The entrance window: every dungeon entrance, with a 3D preview |
| `src/app/export_window.rs` | The Export to Daggerfall Unity window |
| `tests/real_data.rs` | Tests against a real install; set `DAGGERFALL_ARENA2` to run them |

The file formats follow [Daggerfall Unity](https://github.com/Interkarma/daggerfall-unity)
(MIT), which was used as the reference implementation.
