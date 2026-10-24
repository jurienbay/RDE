README.md
=========

The Reasonable Desktop Enviroment is a CDE-lookalike written
entirely in a non-legacy way with zero need for X11 or GL.

The full render path is exclusively Vulkan and is entirely an
authorative Wayland compositor. Additionally dmabuf is leverage
to ensure no CPU overhead. All artefacts are procedurally generated
upon the GPU to ensure the CPU and main system RAM are *NOT* used!

# Why did I write this?

I am sick and tired of wacko UX design where your day to day workflow is this
mental juxtaposition of incongruent ancient alien hieroglyphics and overloaded
irrelevant gestures hoping schizophrenically about the screen.

The 90's designed the CDE desktop around purely pragmatic workflows of getting
a particular job done and leaving time in the day to go outside to see freinds
and touch grass. The aim is to recreate that.

Additionally, why on Earth are we still using the CPU to do tasks that the GPU
should be doing from rendering text glyphs to compositing textures and so on?

Anything graphics shall be done on the GPU!

# Build and Install

dep:
----

```bash
 $ sudo apt install libinput-dev librust-libudev-sys-dev \
librust-libdbus-sys-dev librust-libsystemd-dev libsystemd-dev libshaderc-dev \
libseat-dev libgbm-dev libpixman-1-dev libxkbcommon-dev foot clang libclang-dev \
llvm-dev libpam0g-dev rpm
```

```bash
    $ WAYLAND_DEBUG=server cargo run
```

and in another window run:

```bash
    $ WAYLAND_DISPLAY=wayland-0 foot
```

# AI Policy

Very simple: ['F', 'U', 'C', 'K'] ++ ['O', 'F', 'F'].
