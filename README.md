# Ayagami: Open source 2D puppet rendering SDK

**[Web Demo](https://demo.ayagami.dev)**

[![Github-sponsors](https://img.shields.io/github/sponsors/hoshinolina?label=Sponsor&logo=GitHub)](https://lina.yt/sponsor)
[![Ko-Fi](https://shields.io/badge/ko--fi-Tip-ff5f5f?logo=ko-fi)](https://lina.yt/kofi)

Ayagami (彩紙) is a 2D puppet model loading and rendering library written in Rust. It is designed to be compatible with models in the Live2D model format, while being extensible to new model formats and features in the future.

**Ayagami is completely free software dual licensed under the MIT and Apache2 licenses**. You may use it for any purpose as long as you abide by your choice of either license, without having to pay any royalties or obtain permission from anyone. This includes any use cases, **including both games with built-in models and expandable applications that load user-provided models**.

This software is developed strictly **using black-box reverse engineering only**. That means that it is a complete, from-scratch, independent implementation, and no license terms were violated during its development. **We have never and will never disassemble or decompile any proprietary software in order to develop this project**. To guarantee this, we have a strict [contributor policy](CONTRIBUTING.md).

## Demo

Check out the [Web demo & model poser](https://demo.ayagami.dev)! This demo runs entirely in your browser, and your model data is not sent anywhere outside your machine. You can use it to manually pose your model (high quality screenshot feature coming soon).

You can also build and run the demo as a native app by running `cargo run -r` in the `ayagami-demo` directory. Use `trunk serve --release` instead to run the web version locally (using [trunk](https://trunk-rs.github.io/trunk/)).

## Status

The API is pretty unstable and subject to change, and there is no documentation yet! crates.io release coming soon. For now, take a look at `ayagami-demo` for a usage example.

The code is messy; I'm releasing this early to get feedback on the API and start enabling users, but expect significant cleanup over time.

### Supported features

* MOC3 file loading & features up to SDK 5.0: Parts, ArtMeshes, Rotation & Warp deformers, Glue, Blendshape Parameters, etc.
* Computing the final ArtMeshes given a model & parameters: `driver`
  * Live2D-equivalent interpolation, extrapolation, and deformer chaining algorithms
* Reference renderer using [wgpu-rs](https://wgpu.rs): `ayagami-render` (supports Vulkan, Metal, DirectX, OpenGL, WebGL/WebGPU backends):
  * Partial recomputation of only changed model portions/masks when parameters change
  * Skipping computation of invisible items
  * 1:1 pixel quality masks
  * Supports linear and sRGB (gamma space) blending color modes (selectable depending on color attachment config)
  * Premultiplied alpha and correct texture sampling (no weird edges like VTube Studio).
  * Multithreaded texture decoding & GPU optimized color conversion, for fast model loading.
* Demo app built on [egui](https://egui.rs) (web & native): `ayagami-demo`
  * Support for loading model metadata (model3, cdi3), model and textures from a ZIP archive
  * sRGB (gamma space) blending for now due to egui limitations, hoping to switch to linear light in the future.

### TODO

* [ ] Document MOC3 file format
* [ ] Screenshot tool in demo poser app
* [ ] Physics engine
* [ ] Expression file support
* [ ] Pose file (part linking?) support
* [ ] Motion file (animation) support
* [ ] Full model file verification (reject models with inconsistencies at load time, see Safety section below)
* [ ] Godot component
* [ ] C compatible API
* [ ] Embeddable web component
* [ ] Mesh deform acceleration via GPU compute
* [ ] SDK 5.3 features (advanced blend & off screen rendering) - need a test model!
* [ ] Position tracking helper features (for object pinning etc.)
* [ ] Conservative, fast bounding box calculations
* [ ] Optimized clipping masks
* [ ] Optimization & `safe-only` feature.

### Safety & security 

Ayagami is currently written in 100% pure rust (other than a tiny code path in the optimized vertex buffer blending code which can be trivially proven to be sound). This means that an invalid model cannot cause undefined behavior or lead to an exploitable vulnerability other than DoS. Currently there is limited model validation, which means that an inconsistent model (or a bug) may cause the library to panic at runtime. This will be improved in the future to proactively and gracefully reject inconsistent models at load time.

In the future we plan to introduce carefully controlled and limited scope `unsafe` code to speed up some of the codebase (such as eliding bounds checks on object field array accesses when the object index is already known to be in bounds). While this is not expected to lead to any vulnerabilities (it will always be a design goal to have zero UB regardless of input, and unsafe code will be wrapped in safe abstractions), it will be done by introducing a `safe-only` feature so that paranoid users can opt in to disabling the unsafe code and trade off performance for absolute memory safety certainty. 

## Architecture

No docs yet, so here's a quick overview:

* `ayagami::core`: Core traits that describe a puppet model (rig only, not textures nor auxiliary data). These traits make the rest of the codebase generic over the model format, to allow new formats and in-memory representations to be used on the future (including potentially editing tools).
* `ayagami::file`: Packed (MOC3) file loader. This uses a bunch of gnarly macros to automate generating accessors for file objects and properties, while keeping the struct-of-arrays data organization of the on-disk file in memory, based on a high level description of the file objects (`ayagami::file::classes`). As the API is largely automatically generated, it is fairly obtuse and not documented, but it may be used to access the raw data model of MOC3 files if desired.
* `ayagami::file::model` bridges the raw data model and higher level traits, providing a cleaner abstraction over the model data. This elides implementation specific data (likely intended for the original implementation, but not very useful/safe to rely on).
* `ayagami::driver`: An API that builds on the `core` traits to compute model positions and deformations for a given pose. Essentially: model and parameters in, deformed ArtMeshes (draw objects) out. The core algorithms that implement the behavior of the models live here, including subtle ones like deformer interpolation/extrapolation.
* `ayagami-render`: A reference model renderer implementation, intended to be high quality, cross platform, efficient for desktop/VTubing use cases, and easy to integrate anywhere that `wgpu` can run on. This implementation is suitable for engines that can run arbitrary GPU rendering code inside their render loop, or it can also be used to draw into an offscreen texture. Users who need more advanced features such as custom shaders, additional texture layers (emissive, normals, etc.), between-layer item rendering, effects, or tight integration into an existing engine are encouraged to use this renderer as a reference and either fork it or develop their own, using the `driver` API.
* `ayagami-demo`: A demo and test app for the rest of the stack, built using egui. This is what runs on [demo.ayagami.dev](https://demo.ayagami.dev). It lets you load a model, pan/zoom freely, and adjust all parameter values.

## Contributing

PRs are not currently accepted, as the codebase is in quick flux and we want to make sure the reverse engineering work is [completed entirely in public and with full transparency](https://www.youtube.com/watch?v=eEa0-wt1SqE&list=PL8S0yfRexZULmW9fhF2gcMsuPBi6jeVhZ). However, if you have feature requests or bug reports, please feel free to file an issue!

## FAQ

### Is this legal?

Yes. As Ayagami is completely original software, it is just like any other piece of open source software.

### How did you pull this off?

Black-box reverse engineering! That means looking at Live2D model (MOC3) files with a hex editor to understand their structure, and then manually creating test models and feeding them into a test application (in this case, VTube Studio) to observe how they render.

See also Lina's [Live2D Reverse Engineering FAQ](https://lina.yt/moc3faq).

## Legal & Trademark Notice

Ayagami is an independent, community-developed project.  It is **not affiliated with, authorized by, endorsed by, or sponsored by Live2D Inc.** (formerly Cybernoids Co., Ltd.) or any of its affiliates.

"Live2D®" and "Cubism" are trademarks of Live2D Inc. in Japan and/or other countries; all other trademarks are the property of their respective owners.  These marks are used here only nominatively, to describe file-format compatibility, and **no endorsement, affiliation, or sponsorship is implied.**
