# Katkı

[English](CONTRIBUTING.md) · **Türkçe**

Katkıya açığız. Hata bildirimi ve öneriler için [issue](https://github.com/emrebal98/yaltri/issues) açabilirsin.

## Ortamı kurmak

1. [Rust](https://rustup.rs)'ı kur (stable).
2. Linux'taysan derleme bağımlılıklarını kur:
   ```sh
   # Ubuntu / Debian
   sudo apt install build-essential pkg-config libxkbcommon-dev libxkbcommon-x11-dev \
     libwayland-dev libx11-dev libx11-xcb-dev libxcb1-dev libfontconfig-dev \
     libfreetype-dev libvulkan-dev libdbus-1-dev
   # Arch
   sudo pacman -S base-devel libxkbcommon-x11 wayland libx11 libxcb fontconfig \
     freetype2 vulkan-icd-loader dbus
   ```
3. Repoyu klonla ve çalıştır:
   ```sh
   git clone https://github.com/emrebal98/yaltri
   cd yaltri
   cargo run -p yaltri
   ```

## Sık kullanılan komutlar

```sh
cargo run -p yaltri                      # debug build, loglar terminalde
cargo test --workspace                   # testler
cargo clippy --workspace --all-targets   # lint
cargo build --release -p yaltri          # target/release/yaltri(.exe)
```

Ayrıntılı log için `YALTRI_LOG=debug` ortam değişkenini ayarla.

## Proje yapısı

```
crates/
  yaltri/           uygulama: arama penceresi, renk seçici, editör, ayarlar (GPUI)
  yaltri-core/      ayarlar, komutlar, arama altyapısı, geçmiş
  yaltri-calc/      hesap makinesi
  yaltri-files/     dosya index'i ve değişiklik takibi
  yaltri-apps/      uygulama ve oyun keşfi, ikonlar
  yaltri-color/     renk dönüşümleri, formatlar, renk adları
  yaltri-platform/  tepsi, global kısayollar, ekran yakalama, işletim sistemi bağlantıları
```

Arayüz [GPUI](https://gpui.rs) ile yazıldı. GPUI henüz 1.0 olmadığı için sürümü `Cargo.toml`'da
sabitlendi (`gpui-kit = "=0.9.1"`); güncellerken ayrıca test et.

## Dokümantasyon dilleri

Her README çevirisi `README.md`'nin yanında `README.<dil>.md` olarak durur (örneğin `README.tr.md`);
bu rehber de aynı şekilde (`CONTRIBUTING.<dil>.md`).
Yeni dil eklemek için `README.md`'yi kopyala, çevir ve her README'nin en üstündeki dil satırına link ekle.

## Katkı akışı

- Her değişiklik kendi branch'inde yapılır ve pull request ile gelir; `main`'e doğrudan push yapılmaz.
- Commit mesajları ve PR başlıkları [Conventional Commits](https://www.conventionalcommits.org) biçiminde ve tamamen küçük harftir:
  `tür(kapsam): özet`, örneğin `feat(launcher): filter results by kind` veya `fix(tray): follow the system theme`.
  Türler: `feat`, `fix`, `refactor`, `perf`, `docs`, `test`, `ci`, `chore`. Kapsam alanı belirtir: `launcher`, `picker`, `settings`, `tray`, `files`, `apps`…
- CI (clippy + testler, Windows ve Linux) pull request'lerde çalışır; ayrı bir kontrol PR başlığını ve commit mesajlarını denetler.
- Yerel kontrolü her klonda bir kez aç, kurala uymayan commit mesajı hemen reddedilsin:
  ```sh
  git config core.hooksPath .githooks
  ```

## Sürüm yayınlamak

Sürümleri `main`'deki PR başlıklarından [release-please](https://github.com/googleapis/release-please) hazırlar:

- Bir `feat`, `fix` ya da `perf` PR'ı merge edilince **`chore(main): release x.y.z`** PR'ı açılır (ya da güncellenir).
  İçinde sıradaki sürüm, yeni `CHANGELOG.md` girdisi ve `Cargo.toml` / `Cargo.lock` içindeki sürüm artışı olur.
  `feat` ortadaki sayıyı (`0.2.0 → 0.3.0`), `fix` ve `perf` sondakini (`0.2.0 → 0.2.1`) artırır.
  `docs`, `chore`, `ci`, `refactor` ve `test` tek başına sürüm çıkarmaz.
- PR'ları merge etmeye devam et; sürüm PR'ı hepsini toplar. Sen onu merge edene kadar hiçbir şey yayınlanmaz.
- Sürüm PR'ı merge edilince sürüm etiketlenir, Windows ve Linux paketleri derlenip
  [sürüme](https://github.com/emrebal98/yaltri/releases) eklenir ve değişiklik listesiyle birlikte yayınlanır.

Yayınlamadan derlemeyi denemek için: **Actions → Release → Run workflow**.
