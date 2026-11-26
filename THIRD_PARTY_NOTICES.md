# Third-Party Notices and License Attributions

**gitRUST** is licensed under the [MIT License](LICENSE).

Compiled binaries of `gitRUST` link against permissive open-source Rust crates from [crates.io](https://crates.io/) and embed default UI typography provided by `epaint_default_fonts` (`egui` project). The UI also embeds Adobe Source Sans 3 and Source Code Pro. This file records the required redistribution notices and explicit license elections for dual-licensed dependencies.

---

## 1. Explicit License Elections for Multi-Licensed Crates

Where upstream crates offer a choice of licenses (`OR` expressions in SPDX), `gitRUST` elects the permissive, MIT-compatible option:

- **`self_cell` (`1.3.0`):** Published under `Apache-2.0 OR GPL-2.0-only`. `gitRUST` elects **`Apache-2.0`**.
- **`r-efi` (`6.0.0`, transitive target dependency):** Published under `MIT OR Apache-2.0 OR LGPL-2.1-or-later`. `gitRUST` elects **`MIT`**.
- **All `MIT OR Apache-2.0` / `Zlib OR Apache-2.0 OR MIT` / `Unlicense OR MIT` / `0BSD OR MIT OR Apache-2.0` crates:** Compatible under **`MIT`** (or `Apache-2.0` / `Zlib` / `BSD` permissive terms).

---

## 2. Embedded Font Assets (`epaint_default_fonts` `0.37.0`)

The existing `egui` / `epaint_default_fonts` fallback configuration (`(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0`) embeds the following four font files into the executable:

1. **`Ubuntu-Light.ttf`**
   - Copyright (c) 2010–2011 Canonical Ltd.
   - Licensed under the **Ubuntu Font Licence, Version 1.0** (`Ubuntu-font-1.0`).
2. **`NotoEmoji-Regular.ttf`**
   - Copyright (c) Google LLC.
   - Licensed under the **SIL Open Font License, Version 1.1** (`OFL-1.1`).
3. **`Hack-Regular.ttf`**
   - Copyright (c) 2018 Source Foundry Authors; Bitstream Vera Sans Mono Copyright (c) 2003 Bitstream, Inc.
   - Licensed under the **MIT License** and **Bitstream Vera License**.
4. **`emoji-icon-font.ttf`**
   - Licensed under the **MIT License**.

Full upstream font license texts are distributed in the `epaint_default_fonts` crate source archive (`crates/epaint_default_fonts/fonts/` in <https://github.com/emilk/egui>) and must accompany binary release archives produced in Stage 10 (`cargo-about` / release bundle notices).

---

## 3. Bundled Adobe Typeface Assets

The original upstream TTFs are embedded with `include_bytes!`. Redistribute the corresponding SIL Open Font License 1.1 notices with packaged binaries.

| Font | Upstream release | Copyright and license |
| --- | --- | --- |
| Source Sans 3, Regular and Semibold | [3.052R](https://github.com/adobe-fonts/source-sans/releases/tag/3.052R) | © 2010–2022 Adobe, Reserved Font Name “Source”. [OFL-1.1](crates/gitrust-ui/assets/SourceSans3-LICENSE.md) |
| Source Code Pro, Regular | [2.042R-u/1.062R-i/1.026R-vf](https://github.com/adobe-fonts/source-code-pro/releases/tag/2.042R-u%2F1.062R-i%2F1.026R-vf) | © 2023 Adobe, Reserved Font Name “Source”. [OFL-1.1](crates/gitrust-ui/assets/SourceCodePro-LICENSE.md) |

The UI icon paths and gitRUST product mark were authored for this project and are covered by the project MIT license.

## 4. Permissive Crate Ecosystem Summary

All external crates in the `Linux`, `Windows`, and `macOS` dependency graphs use OSI/FSF-recognized permissive licenses verified by [`deny.toml`](deny.toml):

- `MIT`
- `Apache-2.0` and `Apache-2.0 WITH LLVM-exception`
- `BSD-2-Clause` and `BSD-3-Clause`
- `ISC`
- `Zlib`
- `0BSD` and `Unlicense`
- `BSL-1.0` (Boost Software License 1.0)
- `Unicode-3.0` (Unicode License v3)
- `OFL-1.1` and `Ubuntu-font-1.0` (embedded default fonts)
