# Yaltri

[English](README.md) · **Türkçe**

*Yaltrı-* (Göktürkçe): parıldamak.

Yaltri, sistem tepsisinde sessizce bekleyen küçük bir yardımcı. Bir kısayolla açılır,
aradığını bulur, işin bitince kaybolur.

- **Ara:** Uygulamalar, dosyalar ve oyunlar tek kutuda. Yazmaya başladığın anda sonuçlar gelir.
- **Hesapla:** `2^10`, `5 km to mi`, `=pi`. Sonuç Enter ile panoya kopyalanır.
- **Renk seç:** Ekranın herhangi bir yerinden renk al, istediğin formatta kopyala.

Windows ve Linux'ta (Ubuntu, Arch) aynı görünür. Tek dosyadır, kurulum gerektirmez, internete bağlanmaz.

[![Release](https://img.shields.io/github/v/release/emrebal98/yaltri?include_prereleases)](https://github.com/emrebal98/yaltri/releases)
[![CI](https://github.com/emrebal98/yaltri/actions/workflows/ci.yml/badge.svg)](https://github.com/emrebal98/yaltri/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

---

## Kurulum

### Windows

1. [**yaltri-x86_64-pc-windows-msvc.zip**](https://github.com/emrebal98/yaltri/releases/latest/download/yaltri-x86_64-pc-windows-msvc.zip) dosyasını indir.
2. İstediğin bir klasöre çıkar.
3. `yaltri.exe`'yi çalıştır. Tepsi ikonu belirir.

> İlk açılışta Windows "bilinmeyen yayıncı" uyarısı gösterebilir: **Ek bilgi → Yine de çalıştır**.

Bilgisayar açılınca başlasın istersen: `Win+R` → `shell:startup` → açılan klasöre `yaltri.exe` kısayolu koy.

### Linux

```sh
curl -LO https://github.com/emrebal98/yaltri/releases/latest/download/yaltri-x86_64-unknown-linux-gnu.tar.gz
tar xzf yaltri-x86_64-unknown-linux-gnu.tar.gz
install -Dm755 yaltri-x86_64-unknown-linux-gnu/yaltri ~/.local/bin/yaltri
yaltri
```

Eksik kütüphane hatası alırsan:

```sh
sudo apt install libxkbcommon-x11-0 libvulkan1 mesa-vulkan-drivers   # Ubuntu
sudo pacman -S libxkbcommon-x11 vulkan-icd-loader                    # Arch
```

GNOME'da tepsi ikonu için AppIndicator eklentisi gerekir (Ubuntu'da hazır gelir).

Oturum açılınca başlasın istersen:

```sh
mkdir -p ~/.config/autostart
printf '[Desktop Entry]\nType=Application\nName=Yaltri\nExec=%s\n' "$HOME/.local/bin/yaltri" \
  > ~/.config/autostart/yaltri.desktop
```

---

## Kullanım

| Kısayol | Ne yapar |
|---|---|
| `Super+Space` | Arama kutusunu açar |
| `Super+Shift+C` | Renk seçiciyi açar |

Kısayolları **Ayarlar** penceresinden değiştirebilirsin (tepsi ikonuna sağ tık → Ayarlar).

### Arama

| Tuş | |
|---|---|
| `↑` `↓` | Sonuçlar arasında gez |
| `Enter` | Aç |
| `Ctrl+Enter` | Dosyanın bulunduğu klasörü aç |
| `Tab` / `Shift+Tab` | Filtre değiştir: Tümü, Uygulamalar, Dosyalar, Oyunlar |
| `Backspace` (kutu boşken) | Filtreyi kaldır |
| `Esc` | Kapat |

Yazarken filtrelemek için başa kısaltma ve boşluk yaz: `/u ` uygulama, `/d ` dosya, `/o ` oyun
(`/a`, `/f`, `/g` de çalışır). Filtre açıkken kutu boşsa o türdeki her şey listelenir, örneğin bütün oyunların.

- **Uygulamalar:** Başlat menüsündeki ve mağazadan kurulan uygulamalar, Linux'ta kurulu masaüstü uygulamaları.
- **Dosyalar:** Yaltri bilgisayarındaki dosya adlarını kendi index'inde tutar, milyonlarca dosyada bile anında bulur.
  İlk tarama birkaç dakika sürebilir, sonra değişiklikleri kendisi takip eder.
- **Oyunlar:** Steam, Xbox / PC Game Pass, Epic Games, Riot Games, GOG, Ubisoft Connect, EA app;
  Linux'ta Steam, Heroic ve Lutris. Oyunlar kendi launcher'ları üzerinden açılır.
- **Hesap:** Matematik, birim çevirme ve sabitler. Sık kullandıkların listede üste çıkar.

### Renk seçici

- İmlecin yanında rengin kodu ve adı görünür.
- **Fare tekerleği** ile büyüteci aç, yakınlaştır, uzaklaştır.
- **Sol tık** rengi seçer ve kopyalar. **Sağ tık** veya `Esc` vazgeçer.
- Seçimden sonra açılan editörde renk geçmişi, açık/koyu tonlar ve 9 format var:
  HEX, RGB, HSL, HSV, OKLCH, CMYK, HWB, Decimal, Float.
  ★ ile işaretlediğin format, bir sonraki seçimde doğrudan kopyalanır.

### Komut satırı

```sh
yaltri                    # arka planda başlat
yaltri toggle launcher    # arama kutusunu aç / kapat
yaltri toggle picker      # renk seçiciyi aç
yaltri settings           # ayarları aç
yaltri quit               # kapat
```

### Wayland

Wayland, uygulamaların kendi kısayolunu tanımlamasına izin vermez. Kısayolu masaüstü ayarlarından ekle,
komut olarak `yaltri toggle launcher` yaz:

- **GNOME:** Ayarlar → Klavye → Klavye Kısayolları → Özel Kısayollar
- **KDE:** Sistem Ayarları → Kısayollar → Özel Kısayollar
- **Hyprland:** `bind = SUPER, SPACE, exec, yaltri toggle launcher`

---

## Ayarlar ve dosyalar

|  | Windows | Linux |
|---|---|---|
| Ayarlar | `%APPDATA%\yaltri\config\settings.toml` | `~/.config/yaltri/settings.toml` |
| Index, geçmiş, loglar | `%LOCALAPPDATA%\yaltri\data` | `~/.local/share/yaltri` |

**Kaldırmak için:** tepsi menüsünden **Çıkış**, sonra `yaltri` dosyasını ve bu iki klasörü sil.

---

## Geliştirme

Katkıya açığız. Hata bildirimi ve öneriler için [issue](https://github.com/emrebal98/yaltri/issues) açabilirsin.

### Ortamı kurmak

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

### Sık kullanılan komutlar

```sh
cargo run -p yaltri                      # debug build, loglar terminalde
cargo test --workspace                   # testler
cargo clippy --workspace --all-targets   # lint
cargo build --release -p yaltri          # target/release/yaltri(.exe)
```

Ayrıntılı log için `YALTRI_LOG=debug` ortam değişkenini ayarla.

### Proje yapısı

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

### Dokümantasyon dilleri

Her README çevirisi bu dosyanın yanında `README.<dil>.md` olarak durur (örneğin `README.tr.md`).
Yeni dil eklemek için `README.md`'yi kopyala, çevir ve her README'nin en üstündeki dil satırına link ekle.

### Katkı akışı

- Her değişiklik kendi branch'inde yapılır ve pull request ile gelir; `main`'e doğrudan push yapılmaz.
- Commit mesajları ve PR başlıkları [Conventional Commits](https://www.conventionalcommits.org) biçiminde ve tamamen küçük harftir:
  `tür(kapsam): özet`, örneğin `feat(launcher): filter results by kind` veya `fix(tray): follow the system theme`.
  Türler: `feat`, `fix`, `refactor`, `perf`, `docs`, `test`, `ci`, `chore`. Kapsam alanı belirtir: `launcher`, `picker`, `settings`, `tray`, `files`, `apps`…
- CI (clippy + testler, Windows ve Linux) pull request'lerde çalışır; ayrı bir kontrol PR başlığını ve commit mesajlarını denetler.
- Yerel kontrolü her klonda bir kez aç, kurala uymayan commit mesajı hemen reddedilsin:
  ```sh
  git config core.hooksPath .githooks
  ```

### Sürüm yayınlamak

1. `Cargo.toml` içinde `[workspace.package] version` değerini artır ve commit'le.
2. Etiketi gönder:
   ```sh
   git tag v0.2.0
   git push origin v0.2.0
   ```

GitHub Actions Windows ve Linux paketlerini derleyip [Releases](https://github.com/emrebal98/yaltri/releases)
sayfasına yükler. `v0.2.0-beta.1` gibi etiketler ön sürüm olarak yayınlanır.

---

## Lisans

[MIT](LICENSE)
