# Changelog

## [0.5.3](https://github.com/emrebal98/yaltri/compare/v0.5.2...v0.5.3) (2026-11-20)


### Performance

* **files:** store each file name once and trim freed memory after scans ([#62](https://github.com/emrebal98/yaltri/issues/62)) ([c5a9b0e](https://github.com/emrebal98/yaltri/commit/c5a9b0ee5faf21bd1ca03a0681684616f061d633))

## [0.5.2](https://github.com/emrebal98/yaltri/compare/v0.5.1...v0.5.2) (2026-11-19)


### Bug Fixes

* **battery:** keep a sleeping mouse in the tray ([#60](https://github.com/emrebal98/yaltri/issues/60)) ([747f173](https://github.com/emrebal98/yaltri/commit/747f173bc2258bb559237c0e5fea6b573af1b76f))
* **battery:** size the card to the devices found after it opens ([#59](https://github.com/emrebal98/yaltri/issues/59)) ([2b44a23](https://github.com/emrebal98/yaltri/commit/2b44a23155aedd2f2dccd108a96c7bdc80d0a533))

## [0.5.1](https://github.com/emrebal98/yaltri/compare/v0.5.0...v0.5.1) (2026-11-19)


### Bug Fixes

* grant battery access in one click and show update progress on the button ([#55](https://github.com/emrebal98/yaltri/issues/55)) ([a1ae2a9](https://github.com/emrebal98/yaltri/commit/a1ae2a954f327542e03742ca16b1a0960cb2a083))

## [0.5.0](https://github.com/emrebal98/yaltri/compare/v0.4.0...v0.5.0) (2026-11-18)


### Features

* read logitech devices over hid++ ([#51](https://github.com/emrebal98/yaltri/issues/51)) ([48c8fca](https://github.com/emrebal98/yaltri/commit/48c8fcae512d5b9fe7d434948faebc205a018408))
* read wireless headsets headsetcontrol supports ([#52](https://github.com/emrebal98/yaltri/issues/52)) ([04c9d0f](https://github.com/emrebal98/yaltri/commit/04c9d0f1391c93d361e621abe6adab52b32e6c46))
* show bluetooth mice, keyboards and headsets ([#50](https://github.com/emrebal98/yaltri/issues/50)) ([c41f115](https://github.com/emrebal98/yaltri/commit/c41f115d3c0ce815318dcbeca2cc0adead118deb))
* show mouse and earbud battery in the tray ([#49](https://github.com/emrebal98/yaltri/issues/49)) ([74cab44](https://github.com/emrebal98/yaltri/commit/74cab44d44c54a06b56f62d01eee8875f83ecb41))

## [0.4.0](https://github.com/emrebal98/yaltri/compare/v0.3.2...v0.4.0) (2026-11-16)


### Features

* **linux:** leave the terminal when started from one ([#43](https://github.com/emrebal98/yaltri/issues/43)) ([b75c6b8](https://github.com/emrebal98/yaltri/commit/b75c6b8b21f63b535fce7e5ba85bb534f469544a))


### Bug Fixes

* **linux:** restore autostart after a reinstall ([#42](https://github.com/emrebal98/yaltri/issues/42)) ([63adc13](https://github.com/emrebal98/yaltri/commit/63adc1323eea590a958dfb7816c6c5b57e646277))

## [0.3.2](https://github.com/emrebal98/yaltri/compare/v0.3.1...v0.3.2) (2026-11-16)


### Bug Fixes

* **ipc:** don't fail a quit when the daemon drops our ping ([#40](https://github.com/emrebal98/yaltri/issues/40)) ([af67cca](https://github.com/emrebal98/yaltri/commit/af67ccadd117d3dc3758a05f0a9cdf0502f6b17e))

## [0.3.1](https://github.com/emrebal98/yaltri/compare/v0.3.0...v0.3.1) (2026-11-16)


### Bug Fixes

* **linux:** clean up launcher corners and line up tray shortcuts ([#38](https://github.com/emrebal98/yaltri/issues/38)) ([182128d](https://github.com/emrebal98/yaltri/commit/182128d70fc98ebb879976ba4af502a0c4f431a7))

## [0.3.0](https://github.com/emrebal98/yaltri/compare/v0.2.0...v0.3.0) (2026-11-15)


### Features

* **update:** download updates in the background and install them in one click ([#25](https://github.com/emrebal98/yaltri/issues/25)) ([68e38c6](https://github.com/emrebal98/yaltri/commit/68e38c6683122ad534a2adef7eab70415ae426df))

## [0.2.0](https://github.com/emrebal98/yaltri/compare/v0.1.0...v0.2.0) (2026-11-15)


### Features

* **install:** install itself on windows and add a linux install script ([#11](https://github.com/emrebal98/yaltri/issues/11)) ([51874ae](https://github.com/emrebal98/yaltri/commit/51874ae3f37f88ec1f78cd1c6544b1fef29fe866))
* **shortcuts:** global shortcuts on kde, hyprland and other wayland desktops ([#19](https://github.com/emrebal98/yaltri/issues/19)) ([f4539eb](https://github.com/emrebal98/yaltri/commit/f4539eb9150643c0589e407ff087e642accb5adc))
* **uninstall:** uninstall on linux too, with an optional --purge ([#18](https://github.com/emrebal98/yaltri/issues/18)) ([5cebed0](https://github.com/emrebal98/yaltri/commit/5cebed0b04d92aa56eeca9770a88fa3c6d9d3732))


### Bug Fixes

* **picker:** name colors in the ui language ([#13](https://github.com/emrebal98/yaltri/issues/13)) ([4b3a9e2](https://github.com/emrebal98/yaltri/commit/4b3a9e28116d95f647ec0c5a402ec855ae62f2f3))

## 0.1.0 (2026-11-15)


### Features

* **launcher:** rotate through every filter shorthand in the hints ([#3](https://github.com/emrebal98/yaltri/issues/3)) ([9de6e72](https://github.com/emrebal98/yaltri/commit/9de6e72730bf9e1eba0019ed2a6d2fe4995e71db))
* **picker:** open settings at the picker section from the editor hint ([#8](https://github.com/emrebal98/yaltri/issues/8)) ([9fb8b3c](https://github.com/emrebal98/yaltri/commit/9fb8b3cb6c28368fc1ab18bdd27d86de16134bf4))
* **settings:** general section with autostart, language and theme ([#2](https://github.com/emrebal98/yaltri/issues/2)) ([88ff167](https://github.com/emrebal98/yaltri/commit/88ff167d909e5728f6de270cef75db7be2d026e3))


### Bug Fixes

* **linux:** make windows, tray and shortcuts work on ubuntu ([#5](https://github.com/emrebal98/yaltri/issues/5)) ([8a713a7](https://github.com/emrebal98/yaltri/commit/8a713a76e10f48832b8d0ac8e6d4c576a5520d4e))
* **windows:** name the process yaltri and use bitmap icons for small sizes ([#4](https://github.com/emrebal98/yaltri/issues/4)) ([8a49f63](https://github.com/emrebal98/yaltri/commit/8a49f634c340385a76e2d5a62e34fa71435ae521))
* **windows:** use the system ui font on every windows version ([#6](https://github.com/emrebal98/yaltri/issues/6)) ([7e305b1](https://github.com/emrebal98/yaltri/commit/7e305b19a2bfc55a2392098880b5d8aa471fb58c))
