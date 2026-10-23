# Sistem genelinde hızlı menü

## İlk dikey dilim

Uygulama iki pencere kullanır:

- `main`: ayarlar, durum ve normal sürükle-bırak akışı.
- `overlay`: başlangıçta gizli, çerçevesiz, görev çubuğunda görünmeyen ve en üstte
  tutulan 420×420 şeffaf hızlı menü. Bu pencere yalnızca mod seçiciyi, seçili
  dosya önizlemesini ve dairesel hedefleri çizer.

Platform entegrasyonu yalnızca doğrulanmış bir dosya sürüklemesi sırasında
overlay'i seçilen modda açar. Devam eden işletim sistemi sürükle-bırak işlemi
overlay penceresine girdiğinde dosya yolları mevcut dönüşüm ve File Actions
komutlarına aktarılır.

Windows'ta Shift ve Control global kısayol olarak kaydedilmez. Uygulama sol fare
düğmesi basılıyken en az 8 piksel hareketi bir sürükleme adayı olarak kabul eder,
Explorer veya masaüstündeki seçimi doğrular ve yalnızca bu sırada tuşların
durumunu okur. Tuş olayları yutulmaz. Linux'ta modifier tuşlarını gözlemek için
dosya yöneticisi ve GNOME Shell entegrasyonu kullanılır.

## Platform yolları

### Windows

Fare/klavye durumu güvenli Rust API'si üzerinden kısa aralıklarla okunur.
Sol düğmeyle sürükleme başladıktan sonra Shift/Ctrl kenarı menüyü açar veya açık
menünün modunu değiştirir. Overlay, açılış anındaki global imleç konumundan ve
aktif monitör sınırlarından hesaplanan güvenli bir noktaya yerleştirilir; mod
değişiminde konumu korunur. Kaynak pencerenin Explorer veya Windows masaüstü
olması ve gerçek bir Shell dosya seçimi sunması zorunludur.

Windows paketleme işi sabit URL ve SHA-256 değerleriyle FFmpeg ile Poppler'ı
indirir ve Tauri resource olarak NSIS kurulumuna ekler. Çekirdek önce ortam
değişkenlerini ve `PATH`'i, sonra kurulum yanındaki `tools/` dizinini arar.
LibreOffice lisans/boyut nedeniyle paketlenmez; belge ve sunum dönüşümü için
ayrı kurulmalıdır.

### Linux / GNOME

Wayland güvenlik modeli uygulamaların global imleç koordinatını okumasına,
başka bir uygulamadaki seçimi sorgulamasına veya tek başına modifier tuşlarını
genel olarak dinlemesine izin vermez. GNOME dikey dilimi bu yüzden iki dar
entegrasyon kullanır:

- Nautilus `MenuProvider`, geçerli yerel dosya seçimini uygulamanın kullanıcı
  oturumu D-Bus arayüzüne yollar.
- GNOME Shell uzantısı birincil fare düğmesi basılıyken Nautilus veya GNOME
  masaüstünden başlayan sürüklemelerde `Shift`/`Control` tuşlarını gözler.
  İlk `Shift` menüyü açar; sonraki `Shift`/`Control` olayları açık menünün modunu
  değiştirir. İlk açılışta imleç koordinatını alır ve overlay penceresini o
  konumun çevresine taşır. Olayları tüketmez.

Ana uygulama yolları yeniden doğrular, Nautilus seçimini overlay'e olay olarak
aktarır ve menüyü açar. GNOME masaüstünde menü seçim olmadan açılır; gerçek DND
olayı overlay'e girdiğinde dosya yolları yüklenir. Daire dilimleri önceden alınan
veya DND ile gelen seçimle tıklanabilir. GNOME/Wayland üzerinde konumu normal
uygulama değil, yetkili Shell uzantısı compositor API'si üzerinden belirler;
pencere monitörün çalışma alanı dışına taşmayacak şekilde sınırlanır.

## Doğrulama sınırı

GNOME entegrasyonunun D-Bus seçim ve iki menü açma yolları doğrudan çağrılarla
doğrulandı. Shell/Nautilus kullanıcı eklentilerinin gerçek sürükleme testi henüz
kurulum ve etkinleştirme adımını bekliyor.

Windows sürükleme durum makinesi birim testleriyle doğrulandı. Ancak bu Linux
geliştirme makinesinde Explorer, gerçek Windows pencere davranışı, DND teslimi ve
üretilen NSIS kurulumunun uçtan uca çalışması doğrulanamaz; bunlar Windows
makinesindeki kabul testinde kontrol edilmelidir. İzleyici yalnızca Explorer veya
masaüstü Shell görünümünden gerçek bir dosya seçimi okuyabildiğinde menüyü açar.

Kaynaklar:

- [Tauri Window API](https://v2.tauri.app/reference/javascript/api/namespacewindow/)
- [Nautilus MenuProvider](https://gnome.pages.gitlab.gnome.org/nautilus/iface.MenuProvider.html)
