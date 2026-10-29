# Windows kabul testi

Bu kontrol listesi gerçek Windows Explorer ve kurulmuş NSIS paketi üzerinde
çalıştırılmalıdır. Linux'taki çapraz derleme kontrolü bu davranışları kanıtlamaz.

## Kurulum ve hızlı menü

1. GitHub Actions'ın `grapefruit-windows-installer` artifact'ındaki NSIS
   kurulumunu çalıştırın ve Grapefruit'u açın.
   - Başlangıçta “Grapefruit çalışıyor” bildirimi görünmelidir.
   - Bildirim alanının açık bölümünde veya `^` taşma menüsünde Grapefruit simgesi
     bulunmalıdır.
   - Uygulamayı ikinci kez açmak yeni bir arka plan işlemi başlatmamalı; çalışan
     örneğin ana penceresini göstermelidir.
2. Explorer'da bir dosyayı sol tuşla en az birkaç piksel sürükleyin; sürükleme
   devam ederken yalnızca `Shift` tuşuna basın.
3. Çemberin imlecin çevresinde, ekran sınırları içinde ve şeffaf arka planla
   açıldığını doğrulayın.
4. Sol tuşu bırakmadan `Control` tuşuna basın. Menü dosya işlemlerine geçmeli ve
   ilk açıldığı konumda kalmalıdır. Yeniden `Shift` dönüşüm moduna dönmelidir.
5. Dosyayı uygun bir dilime bırakın. Kaynak dosyanın değişmediğini ve yeni
   çıktının aynı klasörde oluştuğunu doğrulayın.

## Dönüşümler

- JPG/PNG/WebP arasında bir görsel dönüşümü yapın.
- Ses içeren bir videoyu WebM, GIF ve MP3'e dönüştürün.
- Bir PDF'i PNG ve TXT'ye dönüştürün. Bu üç grup, kurulumla gelen FFmpeg ve
  Poppler'ı kullanmalı; ayrıca PATH ayarı gerektirmemelidir.
- Yalnızca LibreOffice kurulu sistemde TXT → DOCX, DOCX → PDF ve PPTX → PDF/PNG
  deneyin.
- Yalnızca Microsoft Office kurulu sistemde Word ile TXT → DOCX ve DOCX → PDF,
  PowerPoint ile PPTX → PDF/PNG, Excel ile XLSX → PDF deneyin. Kurulu olmayan
  Office bileşenlerine ait hedefler çemberde görünmemelidir.
- LibreOffice ve Microsoft Office birlikte kuruluysa aynı TXT → DOCX dönüşümünü
  deneyin. Varsayılan motor başarısız olduğunda Grapefruit uyumlu ikinci motora
  geçebilmelidir.
- Uyumlu hiçbir Office motoru yoksa belge, sunum ve tablo dönüşüm hedefleri
  çemberde görünmemelidir; görsel, video ve PDF işlemleri çalışmaya devam etmelidir.
- Aynı dönüşümü iki kez yapın; ikinci çıktı var olan dosyanın üzerine yazmamalı,
  `(1)` ekli yeni ad kullanmalıdır.
- FFmpeg, Poppler, LibreOffice veya Microsoft Office otomasyonu çalışırken
  Komut İstemi/konsol penceresi açılmamalıdır.

## Dosya işlemleri

- Yolu kopyala, üst klasörde göster, kopyala ve taşı işlemlerini ayrı ayrı
  deneyin.
- Birden fazla dosyayı birlikte sürükleyip ortak dönüşüm hedeflerini ve toplu
  dosya işlemlerini kontrol edin.

## Bilinen doğrulama noktası

Windows izleyicisi menüyü yalnızca imlecin altındaki veya foreground durumundaki
Windows Explorer/masaüstü kök penceresinde yerel bir dosya seçimi okunabildiğinde
açar. Aşağıdaki sınırlar özellikle doğrulanmalıdır:

- Windows 11'de sekmeli Explorer penceresinin aktif sekmesindeki dosya
- Windows 10/11 masaüstünde, masaüstü foreground pencere olmasa da sürüklenen dosya
- Tarayıcı, editör ve terminalde metin seçimi yapılırken Shift/Control ile menünün
  açılmaması
