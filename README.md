# Yaltri

*Yaltrı-* (Göktürkçe): parıldamak.

Sistem tepsisinde duran, kısayolla açılan küçük masaüstü araçları: hızlı arama (uygulama,
dosya, oyun, hesap makinesi) ve renk seçici. Windows, Ubuntu ve Arch'ta aynı görünsün diye
[GPUI](https://gpui.rs) ile yazıldı. Tek dosya, kurulum gerektirmez, internete bağlanmaz.

[![CI](https://github.com/emrebal98/yaltri/actions/workflows/ci.yml/badge.svg)](https://github.com/emrebal98/yaltri/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/emrebal98/yaltri?include_prereleases)](https://github.com/emrebal98/yaltri/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Özellikler

**Launcher** (varsayılan `Alt+Space`)

- **Uygulamalar:** Başlat menüsü, Microsoft Store uygulamaları, Linux'ta `.desktop` dosyaları; ikonlarıyla.
- **Dosyalar:** Kendi dosya index'i; Everything gerekmez. ~1,9 milyon dosyada arama ~8 ms.
- **Oyunlar:** Steam, Xbox / PC Game Pass, Epic, Riot, GOG, Ubisoft Connect, EA app; Linux'ta Steam, Heroic, Lutris.
- **Hesap makinesi ve birim çevirme:** `2^10`, `5 km to mi`, `=pi` gibi.
- Sık açtıkların üste çıkar.

**Renk seçici** (varsayılan `Win+Shift+C`)

- Bütün ekranlarda çalışır. Fare tekerleğiyle büyüteci açarsın; büyüteç 4 kademeye kadar büyür.
- Renk adını Türkçe gösterir. Seçilen renk panoya kopyalanır.
- Editörde geçmiş, açık/koyu tonlar ve 9 format var: HEX · RGB · HSL · HSV · OKLCH · CMYK · HWB · Decimal · Float.

**Genel**

- Tray ikonu, açık/koyu temayı sistemden alır.
- Kısayollar ayarlardan değiştirilir. Windows'un veya PowerToys'un kullandığı bir kısayolu seçersen uyarır
  (PowerToys Run da `Alt+Space` kullanır).

## Kurulum

Hazır dosyalar [Releases](https://github.com/emrebal98/yaltri/releases) sayfasında.

### Windows 10 / 11

1. [`yaltri-x86_64-pc-windows-msvc.zip`](https://github.com/emrebal98/yaltri/releases/latest/download/yaltri-x86_64-pc-windows-msvc.zip) dosyasını indir.
2. Zip'i bir klasöre çıkar, örneğin `%LOCALAPPDATA%\Programs\Yaltri`.
3. `yaltri.exe`'yi çalıştır. Tray'de ikon görünür.

Exe imzalı değil, bu yüzden ilk açılışta SmartScreen uyarı verebilir. *Ek bilgi → Yine de çalıştır* ile geçebilirsin.

**Windows açılınca başlasın:** `Win+R` → `shell:startup` yaz. Açılan klasöre `yaltri.exe` için bir kısayol koy.

PowerShell ile aynı işler:

```powershell
$dir = "$env:LOCALAPPDATA\Programs\Yaltri"
Invoke-WebRequest https://github.com/emrebal98/yaltri/releases/latest/download/yaltri-x86_64-pc-windows-msvc.zip -OutFile $env:TEMP\yaltri.zip
Expand-Archive $env:TEMP\yaltri.zip $env:TEMP\yaltri -Force
New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item $env:TEMP\yaltri\*\yaltri.exe $dir -Force
& "$dir\yaltri.exe"
```

### Linux (Ubuntu 22.04+, Arch)

```sh
curl -LO https://github.com/emrebal98/yaltri/releases/latest/download/yaltri-x86_64-unknown-linux-gnu.tar.gz
tar xzf yaltri-x86_64-unknown-linux-gnu.tar.gz
install -Dm755 yaltri-x86_64-unknown-linux-gnu/yaltri ~/.local/bin/yaltri
yaltri
```

Çalışması için gereken kütüphaneler (çoğu masaüstünde zaten kurulu):

```sh
# Ubuntu / Debian
sudo apt install libxkbcommon-x11-0 libvulkan1 mesa-vulkan-drivers
# Arch
sudo pacman -S libxkbcommon-x11 vulkan-icd-loader   # + ekran kartına göre vulkan-intel / vulkan-radeon / nvidia-utils
```

- GNOME'da tray ikonu için AppIndicator eklentisi gerekir. Ubuntu'da varsayılan olarak kurulu. Arch'ta `gnome-shell-extension-appindicator` paketini kur.
- Wayland, uygulamaların kendi global kısayolunu almasına izin vermez. Kısayolu masaüstü ayarlarından bağla, ayrıntılar [aşağıda](#wayland).

**Oturum açılınca başlasın:**

```sh
mkdir -p ~/.config/autostart
cat > ~/.config/autostart/yaltri.desktop <<EOF
[Desktop Entry]
Type=Application
Name=Yaltri
Exec=$HOME/.local/bin/yaltri
X-GNOME-Autostart-enabled=true
EOF
```

### Kaynaktan derleme

[Rust](https://rustup.rs) (stable) gerekir.

```sh
git clone https://github.com/emrebal98/yaltri
cd yaltri
cargo build --release -p yaltri    # çıktı: target/release/yaltri(.exe)
```

Linux'ta derleme için gereken başlık dosyaları:

```sh
# Ubuntu / Debian
sudo apt install build-essential pkg-config libxkbcommon-dev libxkbcommon-x11-dev \
  libwayland-dev libx11-dev libx11-xcb-dev libxcb1-dev libfontconfig-dev libfreetype-dev \
  libvulkan-dev libdbus-1-dev
# Arch
sudo pacman -S base-devel libxkbcommon-x11 wayland libx11 libxcb fontconfig freetype2 vulkan-icd-loader dbus
```

### Kaldırma

Tray menüsünden *Çıkış*. Sonra exe'yi ve şu klasörleri sil:

| | Windows | Linux |
|---|---|---|
| Ayarlar | `%APPDATA%\yaltri` | `~/.config/yaltri` |
| Veri (index, geçmiş, log) | `%LOCALAPPDATA%\yaltri` | `~/.local/share/yaltri` |

## Kullanım

```sh
yaltri                    # arka planda başlatır (tray ikonu)
yaltri toggle launcher    # launcher'ı açar/kapatır
yaltri toggle picker      # renk seçiciyi açar
yaltri settings           # ayarlar penceresini açar
yaltri quit               # kapatır
```

İkinci bir `yaltri` çağrısı yeni kopya açmaz, komutu çalışan uygulamaya iletir. Masaüstü
kısayollarına bu komutları bağlayabilirsin.

### Launcher

| Tuş | İşlev |
|---|---|
| `↑` / `↓` | Seç |
| `Enter` | Aç. Hesap sonucunda sonucu kopyalar |
| `Ctrl+Enter` | Dosyayı klasöründe gösterir |
| `Esc` | Kapatır |

Oyunlar kendi launcher'ları üzerinden başlatılır, böylece giriş, overlay ve DRM normal çalışır.
Bunun için Steam'de `steam://rungameid`, Epic'te `com.epicgames.launcher://`, Ubisoft'ta `uplay://`,
EA'de `origin2://`, Riot'ta Riot Client, Xbox'ta `shell:AppsFolder` kullanılır.
Launcher'lar *Ayarlar → Oyunlar*'dan tek tek açılıp kapatılır.

### Renk seçici

- İmlecin yanında rengin kodu ve adı görünür.
- Tekerleği yukarı çevirince büyüteç açılır, aşağı çevirince küçülüp kapanır.
- Sol tık rengi seçer ve kopyalar. Sağ tık veya `Esc` iptal eder.
- Seçimden sonra editör açılır. Editörde ★ ile işaretlediğin format, seçimde kopyalanan format olur.

Windows'ta ve X11'de seçim anındaki ekran görüntüsü donmuş halde gösterilir. Wayland'de
(GNOME, KDE, Hyprland) ekran okunamadığı için masaüstünün kendi renk seçicisi açılır
(`xdg-desktop-portal` PickColor). Sonuç yine Yaltri'nin editörüne gelir.

### Dosya index'i

Everything gibi yalnızca dosya adlarını RAM'de tutar: ~1,9M dosya yaklaşık 210 MiB yer kaplar.
İlk tarama birkaç saniye ile birkaç dakika arası sürer. Index diske kaydedilir ve sonraki açılışta ~0,1 sn'de yüklenir.
Sonraki taramalarda index sıfırdan kurulmaz, yalnızca diskteki değişiklikler uygulanır.

- **Windows:** Bütün sabit diskler taranır. Değişiklikler anında izlenir; yönetici yetkisi gerekmez.
- **Linux:** `/` taranır. Ev dizini anında izlenir, geri kalanı saatte bir yeniden taranır.
- `C:\Windows`, `/proc`, `node_modules`, `.git` gibi klasörler atlanır. AppData, nokta ile başlayan
  klasörler ve önbellekler index'e girer ama sonuçlarda alta itilir.

## Yapılandırma

Ayarların çoğu ayarlar penceresinden değiştirilir. Dosya olarak:
`%APPDATA%\yaltri\config\settings.toml` (Windows), `~/.config/yaltri/settings.toml` (Linux).

```toml
[launcher]
hotkey = "Alt+Space"
max_results = 8

[picker]
hotkey = "Super+Shift+C"
copy_format = "hex"

[files]
roots = []              # boş bırakırsan bütün diskler (Windows) veya / (Linux) taranır
exclude_paths = []
exclude_names = ["node_modules", ".git", "__pycache__"]

[games]
steam = true
xbox = true
epic = true
```

Log dosyaları: `%LOCALAPPDATA%\yaltri\data\logs` veya `~/.local/share/yaltri/logs`.
Daha ayrıntılı log için `YALTRI_LOG=debug` ortam değişkenini ayarla.

### Wayland

Kısayolu masaüstü ayarlarından `yaltri toggle launcher` komutuna bağla:

- **GNOME:** Ayarlar → Klavye → Klavye Kısayolları → Özel Kısayollar
- **KDE:** Sistem Ayarları → Kısayollar → Özel Kısayollar
- **Hyprland:** `bind = SUPER, SPACE, exec, yaltri toggle launcher`

GNOME'da launcher XWayland üzerinden açılır, Ulauncher da aynı yöntemi kullanır. KDE ve Hyprland'de layer-shell kullanılır.

## Geliştirme

```sh
cargo run -p yaltri       # debug build, konsol logları açık
cargo test --workspace
```

```
crates/
  yaltri-core/      ayarlar, komutlar, arama sözleşmesi, geçmiş (UI'dan bağımsız)
  yaltri-calc/      fend-core tabanlı hesap
  yaltri-files/     dosya index'i: paralel tarama, kompakt isim havuzu, snapshot, değişiklik izleme
  yaltri-apps/      uygulama ve oyun keşfi, ikonlar, eşleştirme
  yaltri-color/     renk dönüşümleri, formatlar, tonlar, renk adları
  yaltri-platform/  tray, global kısayol, tek kopya IPC, ekran yakalama, oturum tespiti
  yaltri/           GPUI uygulaması: launcher, renk seçici, editör, ayarlar
```

GPUI, `gpui-kit = "=0.9.1"` ile tam sürüme sabitlendi. GPUI henüz 1.0 değil ve sürümler arasında
uyumsuz değişiklikler oluyor, bu yüzden güncellemeyi ayrıca test ederek yap.

### Sürüm yayınlama

1. `Cargo.toml` → `[workspace.package] version` değerini yükselt, commit'le.
2. Etiketle ve gönder:
   ```sh
   git tag v0.2.0
   git push origin v0.2.0
   ```
3. [Release workflow'u](.github/workflows/release.yml) Windows zip'ini ve Linux tar.gz'sini derler,
   SHA-256 dosyalarıyla birlikte GitHub Release'e yükler ve sürüm notlarını otomatik oluşturur.
   `-` içeren etiketler (`v0.2.0-beta.1`) ön sürüm olarak yayınlanır.

Workflow'u *Actions → Release → Run workflow* ile elle de başlatabilirsin. Bu durumda dosyalar
yayınlanmaz, yalnızca artifact olarak indirilebilir.

## Lisans

[MIT](LICENSE)
