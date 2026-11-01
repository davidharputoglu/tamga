//! Tables de correspondance des noms de paquets entre distributions.
//! Package-name correspondence tables between distribution families.
//!
//! Colonnes : soname (vide pour un paquet non-bibliothèque), Debian/Ubuntu,
//! Fedora/RHEL, openSUSE, Arch. Une case vide = inconnu.
//! Ces tables couvrent les bibliothèques les plus courantes des applications
//! de bureau ; le reste est déduit (règles de nommage, système local, Repology).

use crate::model::Family;

type Row = (&'static str, &'static str, &'static str, &'static str, &'static str);

#[rustfmt::skip]
pub const TABLE: &[Row] = &[
    // --- base C / C++
    ("libc.so.6", "libc6", "glibc", "glibc", "glibc"),
    ("libm.so.6", "libc6", "glibc", "glibc", "glibc"),
    ("libpthread.so.0", "libc6", "glibc", "glibc", "glibc"),
    ("libdl.so.2", "libc6", "glibc", "glibc", "glibc"),
    ("librt.so.1", "libc6", "glibc", "glibc", "glibc"),
    ("libresolv.so.2", "libc6", "glibc", "glibc", "glibc"),
    ("libutil.so.1", "libc6", "glibc", "glibc", "glibc"),
    ("libstdc++.so.6", "libstdc++6", "libstdc++", "libstdc++6", "gcc-libs"),
    ("libgcc_s.so.1", "libgcc-s1", "libgcc", "libgcc_s1", "gcc-libs"),
    ("libatomic.so.1", "libatomic1", "libatomic", "libatomic1", "gcc-libs"),
    ("libgomp.so.1", "libgomp1", "libgomp", "libgomp1", "gcc-libs"),
    // --- compression, crypto, réseau
    ("libz.so.1", "zlib1g", "zlib", "libz1", "zlib"),
    ("libbz2.so.1.0", "libbz2-1.0", "bzip2-libs", "libbz2-1", "bzip2"),
    ("libbz2.so.1", "libbz2-1.0", "bzip2-libs", "libbz2-1", "bzip2"),
    ("liblzma.so.5", "liblzma5", "xz-libs", "liblzma5", "xz"),
    ("libzstd.so.1", "libzstd1", "libzstd", "libzstd1", "zstd"),
    ("liblz4.so.1", "liblz4-1", "lz4-libs", "liblz4-1", "lz4"),
    ("libbrotlidec.so.1", "libbrotli1", "libbrotli", "libbrotlidec1", "brotli"),
    ("libbrotlienc.so.1", "libbrotli1", "libbrotli", "libbrotlienc1", "brotli"),
    ("libssl.so.3", "libssl3", "openssl-libs", "libopenssl3", "openssl"),
    ("libcrypto.so.3", "libssl3", "openssl-libs", "libopenssl3", "openssl"),
    ("libssl.so.1.1", "libssl1.1", "openssl1.1", "libopenssl1_1", "openssl-1.1"),
    ("libcrypto.so.1.1", "libssl1.1", "openssl1.1", "libopenssl1_1", "openssl-1.1"),
    ("libgnutls.so.30", "libgnutls30", "gnutls", "libgnutls30", "gnutls"),
    ("libgcrypt.so.20", "libgcrypt20", "libgcrypt", "libgcrypt20", "libgcrypt"),
    ("libgpg-error.so.0", "libgpg-error0", "libgpg-error", "libgpg-error0", "libgpg-error"),
    ("libcurl.so.4", "libcurl4", "libcurl", "libcurl4", "curl"),
    ("libcurl-gnutls.so.4", "libcurl3-gnutls", "", "", ""),
    ("libnghttp2.so.14", "libnghttp2-14", "libnghttp2", "libnghttp2-14", "libnghttp2"),
    ("libidn2.so.0", "libidn2-0", "libidn2", "libidn2-0", "libidn2"),
    ("libgssapi_krb5.so.2", "libgssapi-krb5-2", "krb5-libs", "krb5", "krb5"),
    ("libkrb5.so.3", "libkrb5-3", "krb5-libs", "krb5", "krb5"),
    ("libnl-3.so.200", "libnl-3-200", "libnl3", "libnl3-200", "libnl"),
    ("libnl-genl-3.so.200", "libnl-genl-3-200", "libnl3", "libnl3-200", "libnl"),
    ("libsqlite3.so.0", "libsqlite3-0", "sqlite-libs", "libsqlite3-0", "sqlite"),
    ("libsodium.so.23", "libsodium23", "libsodium", "libsodium23", "libsodium"),
    // --- système
    ("libdbus-1.so.3", "libdbus-1-3", "dbus-libs", "libdbus-1-3", "dbus"),
    ("libsystemd.so.0", "libsystemd0", "systemd-libs", "libsystemd0", "systemd-libs"),
    ("libudev.so.1", "libudev1", "systemd-libs", "libudev1", "systemd-libs"),
    ("libexpat.so.1", "libexpat1", "expat", "libexpat1", "expat"),
    ("libxml2.so.2", "libxml2", "libxml2", "libxml2-2", "libxml2"),
    ("libuuid.so.1", "libuuid1", "libuuid", "libuuid1", "util-linux-libs"),
    ("libblkid.so.1", "libblkid1", "libblkid", "libblkid1", "util-linux-libs"),
    ("libmount.so.1", "libmount1", "libmount", "libmount1", "util-linux-libs"),
    ("libselinux.so.1", "libselinux1", "libselinux", "libselinux1", ""),
    ("libffi.so.8", "libffi8", "libffi", "libffi8", "libffi"),
    ("libpcre2-8.so.0", "libpcre2-8-0", "pcre2", "libpcre2-8-0", "pcre2"),
    ("libcap.so.2", "libcap2", "libcap", "libcap2", "libcap"),
    ("libacl.so.1", "libacl1", "libacl", "libacl1", "acl"),
    ("libattr.so.1", "libattr1", "libattr", "libattr1", "attr"),
    ("libtinfo.so.6", "libtinfo6", "ncurses-libs", "libncurses6", "ncurses"),
    ("libncursesw.so.6", "libncursesw6", "ncurses-libs", "libncurses6", "ncurses"),
    ("libreadline.so.8", "libreadline8", "readline", "libreadline8", "readline"),
    ("libusb-1.0.so.0", "libusb-1.0-0", "libusb1", "libusb-1_0-0", "libusb"),
    ("libpcsclite.so.1", "libpcsclite1", "pcsc-lite-libs", "libpcsclite1", "pcsclite"),
    ("libpolkit-gobject-1.so.0", "libpolkit-gobject-1-0", "polkit-libs", "libpolkit-gobject-1-0", "polkit"),
    ("libevdev.so.2", "libevdev2", "libevdev", "libevdev2", "libevdev"),
    ("libinput.so.10", "libinput10", "libinput", "libinput10", "libinput"),
    // --- GLib / GTK / GNOME
    ("libglib-2.0.so.0", "libglib2.0-0", "glib2", "libglib-2_0-0", "glib2"),
    ("libgobject-2.0.so.0", "libglib2.0-0", "glib2", "libgobject-2_0-0", "glib2"),
    ("libgio-2.0.so.0", "libglib2.0-0", "glib2", "libgio-2_0-0", "glib2"),
    ("libgmodule-2.0.so.0", "libglib2.0-0", "glib2", "libgmodule-2_0-0", "glib2"),
    ("libgthread-2.0.so.0", "libglib2.0-0", "glib2", "libgthread-2_0-0", "glib2"),
    ("libgtk-3.so.0", "libgtk-3-0", "gtk3", "libgtk-3-0", "gtk3"),
    ("libgdk-3.so.0", "libgtk-3-0", "gtk3", "libgtk-3-0", "gtk3"),
    ("libgtk-4.so.1", "libgtk-4-1", "gtk4", "libgtk-4-1", "gtk4"),
    ("libgtk-x11-2.0.so.0", "libgtk2.0-0", "gtk2", "libgtk-2_0-0", "gtk2"),
    ("libgdk-x11-2.0.so.0", "libgtk2.0-0", "gtk2", "libgtk-2_0-0", "gtk2"),
    ("libgdk_pixbuf-2.0.so.0", "libgdk-pixbuf-2.0-0", "gdk-pixbuf2", "libgdk_pixbuf-2_0-0", "gdk-pixbuf2"),
    ("libpango-1.0.so.0", "libpango-1.0-0", "pango", "libpango-1_0-0", "pango"),
    ("libpangocairo-1.0.so.0", "libpangocairo-1.0-0", "pango", "libpango-1_0-0", "pango"),
    ("libpangoft2-1.0.so.0", "libpangoft2-1.0-0", "pango", "libpango-1_0-0", "pango"),
    ("libcairo.so.2", "libcairo2", "cairo", "libcairo2", "cairo"),
    ("libcairo-gobject.so.2", "libcairo-gobject2", "cairo-gobject", "libcairo-gobject2", "cairo"),
    ("libatk-1.0.so.0", "libatk1.0-0", "atk", "libatk-1_0-0", "at-spi2-core"),
    ("libatk-bridge-2.0.so.0", "libatk-bridge2.0-0", "at-spi2-atk", "libatk-bridge-2_0-0", "at-spi2-core"),
    ("libatspi.so.0", "libatspi2.0-0", "at-spi2-core", "libatspi0", "at-spi2-core"),
    ("libharfbuzz.so.0", "libharfbuzz0b", "harfbuzz", "libharfbuzz0", "harfbuzz"),
    ("libfribidi.so.0", "libfribidi0", "fribidi", "libfribidi0", "fribidi"),
    ("libepoxy.so.0", "libepoxy0", "libepoxy", "libepoxy0", "libepoxy"),
    ("librsvg-2.so.2", "librsvg2-2", "librsvg2", "librsvg-2-2", "librsvg"),
    ("libsecret-1.so.0", "libsecret-1-0", "libsecret", "libsecret-1-0", "libsecret"),
    ("libnotify.so.4", "libnotify4", "libnotify", "libnotify4", "libnotify"),
    ("libsoup-3.0.so.0", "libsoup-3.0-0", "libsoup3", "libsoup-3_0-0", "libsoup3"),
    ("libsoup-2.4.so.1", "libsoup2.4-1", "libsoup", "libsoup-2_4-1", "libsoup"),
    ("libwebkit2gtk-4.1.so.0", "libwebkit2gtk-4.1-0", "webkit2gtk4.1", "libwebkit2gtk-4_1-0", "webkit2gtk-4.1"),
    ("libjavascriptcoregtk-4.1.so.0", "libjavascriptcoregtk-4.1-0", "javascriptcoregtk4.1", "libjavascriptcoregtk-4_1-0", "webkit2gtk-4.1"),
    ("libwebkit2gtk-4.0.so.37", "libwebkit2gtk-4.0-37", "webkit2gtk4.0", "libwebkit2gtk-4_0-37", "webkit2gtk"),
    ("libappindicator3.so.1", "libappindicator3-1", "libappindicator-gtk3", "libappindicator3-1", "libappindicator-gtk3"),
    ("libayatana-appindicator3.so.1", "libayatana-appindicator3-1", "libayatana-appindicator-gtk3", "libayatana-appindicator3-1", "libayatana-appindicator"),
    ("libgstreamer-1.0.so.0", "libgstreamer1.0-0", "gstreamer1", "libgstreamer-1_0-0", "gstreamer"),
    ("libgstbase-1.0.so.0", "libgstreamer1.0-0", "gstreamer1", "libgstreamer-1_0-0", "gstreamer"),
    ("libgstapp-1.0.so.0", "libgstreamer-plugins-base1.0-0", "gstreamer1-plugins-base", "libgstapp-1_0-0", "gst-plugins-base-libs"),
    ("libgstvideo-1.0.so.0", "libgstreamer-plugins-base1.0-0", "gstreamer1-plugins-base", "libgstvideo-1_0-0", "gst-plugins-base-libs"),
    ("libgstaudio-1.0.so.0", "libgstreamer-plugins-base1.0-0", "gstreamer1-plugins-base", "libgstaudio-1_0-0", "gst-plugins-base-libs"),
    ("libgstpbutils-1.0.so.0", "libgstreamer-plugins-base1.0-0", "gstreamer1-plugins-base", "libgstpbutils-1_0-0", "gst-plugins-base-libs"),
    // --- images, polices
    ("libfreetype.so.6", "libfreetype6", "freetype", "libfreetype6", "freetype2"),
    ("libfontconfig.so.1", "libfontconfig1", "fontconfig", "libfontconfig1", "fontconfig"),
    ("libpng16.so.16", "libpng16-16", "libpng", "libpng16-16", "libpng"),
    ("libjpeg.so.8", "libjpeg8", "", "libjpeg8", "libjpeg-turbo"),
    ("libjpeg.so.62", "libjpeg62-turbo", "libjpeg-turbo", "libjpeg62", ""),
    ("libtiff.so.6", "libtiff6", "libtiff", "libtiff6", "libtiff"),
    ("libwebp.so.7", "libwebp7", "libwebp", "libwebp7", "libwebp"),
    // --- X11 / XCB / Wayland / OpenGL
    ("libX11.so.6", "libx11-6", "libX11", "libX11-6", "libx11"),
    ("libX11-xcb.so.1", "libx11-xcb1", "libX11-xcb", "libX11-xcb1", "libx11"),
    ("libxcb.so.1", "libxcb1", "libxcb", "libxcb1", "libxcb"),
    ("libxcb-shm.so.0", "libxcb-shm0", "libxcb", "libxcb-shm0", "libxcb"),
    ("libxcb-render.so.0", "libxcb-render0", "libxcb", "libxcb-render0", "libxcb"),
    ("libxcb-randr.so.0", "libxcb-randr0", "libxcb", "libxcb-randr0", "libxcb"),
    ("libxcb-xfixes.so.0", "libxcb-xfixes0", "libxcb", "libxcb-xfixes0", "libxcb"),
    ("libxcb-shape.so.0", "libxcb-shape0", "libxcb", "libxcb-shape0", "libxcb"),
    ("libxcb-sync.so.1", "libxcb-sync1", "libxcb", "libxcb-sync1", "libxcb"),
    ("libxcb-xkb.so.1", "libxcb-xkb1", "libxcb", "libxcb-xkb1", "libxcb"),
    ("libxcb-dri3.so.0", "libxcb-dri3-0", "libxcb", "libxcb-dri3-0", "libxcb"),
    ("libxcb-present.so.0", "libxcb-present0", "libxcb", "libxcb-present0", "libxcb"),
    ("libxcb-glx.so.0", "libxcb-glx0", "libxcb", "libxcb-glx0", "libxcb"),
    ("libxcb-keysyms.so.1", "libxcb-keysyms1", "xcb-util-keysyms", "libxcb-keysyms1", "xcb-util-keysyms"),
    ("libxcb-icccm.so.4", "libxcb-icccm4", "xcb-util-wm", "libxcb-icccm4", "xcb-util-wm"),
    ("libxcb-image.so.0", "libxcb-image0", "xcb-util-image", "libxcb-image0", "xcb-util-image"),
    ("libxcb-util.so.1", "libxcb-util1", "xcb-util", "libxcb-util1", "xcb-util"),
    ("libxcb-cursor.so.0", "libxcb-cursor0", "xcb-util-cursor", "libxcb-cursor0", "xcb-util-cursor"),
    ("libxcb-render-util.so.0", "libxcb-render-util0", "xcb-util-renderutil", "libxcb-render-util0", "xcb-util-renderutil"),
    ("libXext.so.6", "libxext6", "libXext", "libXext6", "libxext"),
    ("libXrender.so.1", "libxrender1", "libXrender", "libXrender1", "libxrender"),
    ("libXrandr.so.2", "libxrandr2", "libXrandr", "libXrandr2", "libxrandr"),
    ("libXi.so.6", "libxi6", "libXi", "libXi6", "libxi"),
    ("libXfixes.so.3", "libxfixes3", "libXfixes", "libXfixes3", "libxfixes"),
    ("libXdamage.so.1", "libxdamage1", "libXdamage", "libXdamage1", "libxdamage"),
    ("libXcomposite.so.1", "libxcomposite1", "libXcomposite", "libXcomposite1", "libxcomposite"),
    ("libXcursor.so.1", "libxcursor1", "libXcursor", "libXcursor1", "libxcursor"),
    ("libXinerama.so.1", "libxinerama1", "libXinerama", "libXinerama1", "libxinerama"),
    ("libXtst.so.6", "libxtst6", "libXtst", "libXtst6", "libxtst"),
    ("libXss.so.1", "libxss1", "libXScrnSaver", "libXss1", "libxss"),
    ("libXxf86vm.so.1", "libxxf86vm1", "libXxf86vm", "libXxf86vm1", "libxxf86vm"),
    ("libXmu.so.6", "libxmu6", "libXmu", "libXmu6", "libxmu"),
    ("libXaw.so.7", "libxaw7", "libXaw", "libXaw7", "libxaw"),
    ("libXpm.so.4", "libxpm4", "libXpm", "libXpm4", "libxpm"),
    ("libXv.so.1", "libxv1", "libXv", "libXv1", "libxv"),
    ("libXres.so.1", "libxres1", "libXres", "libXRes1", "libxres"),
    ("libutempter.so.0", "libutempter0", "libutempter", "libutempter0", "libutempter"),
    ("libXt.so.6", "libxt6", "libXt", "libXt6", "libxt"),
    ("libXft.so.2", "libxft2", "libXft", "libXft2", "libxft"),
    ("libXau.so.6", "libxau6", "libXau", "libXau6", "libxau"),
    ("libXdmcp.so.6", "libxdmcp6", "libXdmcp", "libXdmcp6", "libxdmcp"),
    ("libSM.so.6", "libsm6", "libSM", "libSM6", "libsm"),
    ("libICE.so.6", "libice6", "libICE", "libICE6", "libice"),
    ("libxkbcommon.so.0", "libxkbcommon0", "libxkbcommon", "libxkbcommon0", "libxkbcommon"),
    ("libxkbcommon-x11.so.0", "libxkbcommon-x11-0", "libxkbcommon-x11", "libxkbcommon-x11-0", "libxkbcommon-x11"),
    ("libxkbfile.so.1", "libxkbfile1", "libxkbfile", "libxkbfile1", "libxkbfile"),
    ("libxshmfence.so.1", "libxshmfence1", "libxshmfence", "libxshmfence1", "libxshmfence"),
    ("libdrm.so.2", "libdrm2", "libdrm", "libdrm2", "libdrm"),
    ("libgbm.so.1", "libgbm1", "mesa-libgbm", "libgbm1", "mesa"),
    ("libGL.so.1", "libgl1", "libglvnd-glx", "libGL1", "libglvnd"),
    ("libGLX.so.0", "libglx0", "libglvnd-glx", "libGLX0", "libglvnd"),
    ("libOpenGL.so.0", "libopengl0", "libglvnd-opengl", "libOpenGL0", "libglvnd"),
    ("libEGL.so.1", "libegl1", "libglvnd-egl", "libEGL1", "libglvnd"),
    ("libGLESv2.so.2", "libgles2", "libglvnd-gles", "libGLESv2-2", "libglvnd"),
    ("libvulkan.so.1", "libvulkan1", "vulkan-loader", "libvulkan1", "vulkan-icd-loader"),
    ("libwayland-client.so.0", "libwayland-client0", "libwayland-client", "libwayland-client0", "wayland"),
    ("libwayland-cursor.so.0", "libwayland-cursor0", "libwayland-cursor", "libwayland-cursor0", "wayland"),
    ("libwayland-egl.so.1", "libwayland-egl1", "libwayland-egl", "libwayland-egl1", "wayland"),
    ("libwayland-server.so.0", "libwayland-server0", "libwayland-server", "libwayland-server0", "wayland"),
    // --- son, multimédia, jeux
    ("libasound.so.2", "libasound2", "alsa-lib", "libasound2", "alsa-lib"),
    ("libpulse.so.0", "libpulse0", "pulseaudio-libs", "libpulse0", "libpulse"),
    ("libpulse-simple.so.0", "libpulse0", "pulseaudio-libs", "libpulse0", "libpulse"),
    ("libpipewire-0.3.so.0", "libpipewire-0.3-0", "pipewire-libs", "libpipewire-0_3-0", "libpipewire"),
    ("libjack.so.0", "libjack-jackd2-0", "pipewire-jack-audio-connection-kit-libs", "libjack0", "jack2"),
    ("libSDL2-2.0.so.0", "libsdl2-2.0-0", "SDL2", "libSDL2-2_0-0", "sdl2"),
    ("libSDL3.so.0", "libsdl3-0", "SDL3", "libSDL3-0", "sdl3"),
    ("libopenal.so.1", "libopenal1", "openal-soft", "libopenal1", "openal"),
    ("libvorbis.so.0", "libvorbis0a", "libvorbis", "libvorbis0", "libvorbis"),
    ("libvorbisfile.so.3", "libvorbisfile3", "libvorbis", "libvorbisfile3", "libvorbis"),
    ("libogg.so.0", "libogg0", "libogg", "libogg0", "libogg"),
    ("libopus.so.0", "libopus0", "opus", "libopus0", "opus"),
    ("libsndfile.so.1", "libsndfile1", "libsndfile", "libsndfile1", "libsndfile"),
    ("libcups.so.2", "libcups2", "cups-libs", "libcups2", "libcups"),
    // --- Mozilla NSS (Electron, navigateurs)
    ("libnss3.so", "libnss3", "nss", "mozilla-nss", "nss"),
    ("libnssutil3.so", "libnss3", "nss-util", "mozilla-nss", "nss"),
    ("libsmime3.so", "libnss3", "nss", "mozilla-nss", "nss"),
    ("libnspr4.so", "libnspr4", "nspr", "mozilla-nspr", "nspr"),
    ("libplc4.so", "libnspr4", "nspr", "mozilla-nspr", "nspr"),
    ("libplds4.so", "libnspr4", "nspr", "mozilla-nspr", "nspr"),
    // --- Qt
    ("libQt5Core.so.5", "libqt5core5a", "qt5-qtbase", "libQt5Core5", "qt5-base"),
    ("libQt5Gui.so.5", "libqt5gui5", "qt5-qtbase-gui", "libQt5Gui5", "qt5-base"),
    ("libQt5Widgets.so.5", "libqt5widgets5", "qt5-qtbase-gui", "libQt5Widgets5", "qt5-base"),
    ("libQt5Network.so.5", "libqt5network5", "qt5-qtbase", "libQt5Network5", "qt5-base"),
    ("libQt5DBus.so.5", "libqt5dbus5", "qt5-qtbase", "libQt5DBus5", "qt5-base"),
    ("libQt5Xml.so.5", "libqt5xml5", "qt5-qtbase", "libQt5Xml5", "qt5-base"),
    ("libQt5Svg.so.5", "libqt5svg5", "qt5-qtsvg", "libQt5Svg5", "qt5-svg"),
    ("libQt6Core.so.6", "libqt6core6", "qt6-qtbase", "libQt6Core6", "qt6-base"),
    ("libQt6Gui.so.6", "libqt6gui6", "qt6-qtbase-gui", "libQt6Gui6", "qt6-base"),
    ("libQt6Widgets.so.6", "libqt6widgets6", "qt6-qtbase-gui", "libQt6Widgets6", "qt6-base"),
    ("libQt6Network.so.6", "libqt6network6", "qt6-qtbase", "libQt6Network6", "qt6-base"),
    ("libQt6DBus.so.6", "libqt6dbus6", "qt6-qtbase", "libQt6DBus6", "qt6-base"),
    ("libQt6Svg.so.6", "libqt6svg6", "qt6-qtsvg", "libQt6Svg6", "qt6-svg"),
    // --- ajouts : réseau, annuaire, divers
    ("libldap.so.2", "libldap2", "openldap", "libldap2", "libldap"),
    ("liblber.so.2", "libldap2", "openldap", "libldap2", "libldap"),
    ("libpsl.so.5", "libpsl5", "libpsl", "libpsl5", "libpsl"),
    ("librtmp.so.1", "librtmp1", "librtmp", "librtmp1", "rtmpdump"),
    ("libssh.so.4", "libssh-4", "libssh", "libssh4", "libssh"),
    ("libssh2.so.1", "libssh2-1", "libssh2", "libssh2-1", "libssh2"),
    ("libsasl2.so.2", "libsasl2-2", "cyrus-sasl-lib", "libsasl2-3", "libsasl"),
    ("libk5crypto.so.3", "libk5crypto3", "krb5-libs", "krb5", "krb5"),
    ("libcom_err.so.2", "libcom-err2", "libcom_err", "libcom_err2", "e2fsprogs"),
    ("libkeyutils.so.1", "libkeyutils1", "keyutils-libs", "libkeyutils1", "keyutils"),
    ("libgmp.so.10", "libgmp10", "gmp", "libgmp10", "gmp"),
    ("libnettle.so.8", "libnettle8", "nettle", "libnettle8", "nettle"),
    ("libhogweed.so.6", "libhogweed6", "nettle", "libhogweed6", "nettle"),
    ("libp11-kit.so.0", "libp11-kit0", "p11-kit", "libp11-kit0", "p11-kit"),
    ("libtasn1.so.6", "libtasn1-6", "libtasn1", "libtasn1-6", "libtasn1"),
    ("libunistring.so.5", "libunistring5", "libunistring", "libunistring5", "libunistring"),
    ("libelf.so.1", "libelf1", "elfutils-libelf", "libelf1", "libelf"),
    ("libjson-c.so.5", "libjson-c5", "json-c", "libjson-c5", "json-c"),
    ("libyaml-0.so.2", "libyaml-0-2", "libyaml", "libyaml-0-2", "libyaml"),
    ("libarchive.so.13", "libarchive13", "libarchive", "libarchive13", "libarchive"),
    ("libmagic.so.1", "libmagic1", "file-libs", "libmagic1", "file"),
    ("libpopt.so.0", "libpopt0", "popt", "libpopt0", "popt"),
    ("libgpgme.so.11", "libgpgme11", "gpgme", "libgpgme11", "gpgme"),
    ("libassuan.so.0", "libassuan0", "libassuan", "libassuan0", "libassuan"),
    ("libpcap.so.0.8", "libpcap0.8", "libpcap", "libpcap1", "libpcap"),
    ("libsensors.so.5", "libsensors5", "lm_sensors-libs", "libsensors4", "lm_sensors"),
    ("libgirepository-1.0.so.1", "libgirepository-1.0-1", "gobject-introspection", "libgirepository-1_0-1", "gobject-introspection-runtime"),
    ("libjson-glib-1.0.so.0", "libjson-glib-1.0-0", "json-glib", "libjson-glib-1_0-0", "json-glib"),
    ("libgudev-1.0.so.0", "libgudev-1.0-0", "libgudev", "libgudev-1_0-0", "libgudev"),
    ("libadwaita-1.so.0", "libadwaita-1-0", "libadwaita", "libadwaita-1-0", "libadwaita"),
    ("libvte-2.91.so.0", "libvte-2.91-0", "vte291", "libvte-2_91-0", "vte3"),
    ("libgtksourceview-4.so.0", "libgtksourceview-4-0", "gtksourceview4", "libgtksourceview-4-0", "gtksourceview4"),
    ("libgtksourceview-5.so.0", "libgtksourceview-5-0", "gtksourceview5", "libgtksourceview-5-0", "gtksourceview5"),
    ("libcanberra.so.0", "libcanberra0", "libcanberra", "libcanberra0", "libcanberra"),
    ("libcanberra-gtk3.so.0", "libcanberra-gtk3-0", "libcanberra-gtk3", "libcanberra-gtk3-0", "libcanberra"),
    ("libportaudio.so.2", "libportaudio2", "portaudio", "libportaudio2", "portaudio"),
    ("libsamplerate.so.0", "libsamplerate0", "libsamplerate", "libsamplerate0", "libsamplerate"),
    ("libFLAC.so.12", "libflac12", "flac-libs", "libFLAC12", "flac"),
    ("libmp3lame.so.0", "libmp3lame0", "lame-libs", "libmp3lame0", "lame"),
    ("libgdbm.so.6", "libgdbm6", "gdbm", "libgdbm6", "gdbm"),
    ("libtirpc.so.3", "libtirpc3", "libtirpc", "libtirpc3", "libtirpc"),
    ("libnsl.so.3", "libnsl2", "libnsl2", "libnsl3", "libnsl"),
    ("libcrypt.so.1", "libcrypt1", "libxcrypt", "libcrypt1", "libxcrypt"),
    ("libpam.so.0", "libpam0g", "pam", "pam", "pam"),
    ("libaudit.so.1", "libaudit1", "audit-libs", "libaudit1", "audit"),
    ("libseccomp.so.2", "libseccomp2", "libseccomp", "libseccomp2", "libseccomp"),
    ("libgnutls-openssl.so.27", "libgnutls-openssl27", "gnutls", "libgnutls-openssl27", "gnutls"),
    ("libSDL2_image-2.0.so.0", "libsdl2-image-2.0-0", "SDL2_image", "libSDL2_image-2_0-0", "sdl2_image"),
    ("libSDL2_mixer-2.0.so.0", "libsdl2-mixer-2.0-0", "SDL2_mixer", "libSDL2_mixer-2_0-0", "sdl2_mixer"),
    ("libSDL2_ttf-2.0.so.0", "libsdl2-ttf-2.0-0", "SDL2_ttf", "libSDL2_ttf-2_0-0", "sdl2_ttf"),
    ("libGLU.so.1", "libglu1-mesa", "mesa-libGLU", "libGLU1", "glu"),
    ("libglut.so.3", "freeglut3", "freeglut", "libglut3", "freeglut"),
    // --- paquets sans bibliothèque (commandes, données, interpréteurs)
    ("", "bash", "bash", "bash", "bash"),
    ("", "coreutils", "coreutils", "coreutils", "coreutils"),
    ("", "sed", "sed", "sed", "sed"),
    ("", "grep", "grep", "grep", "grep"),
    ("", "gawk", "gawk", "gawk", "gawk"),
    ("", "findutils", "findutils", "findutils", "findutils"),
    ("", "tar", "tar", "tar", "tar"),
    ("", "gzip", "gzip", "gzip", "gzip"),
    ("", "xz-utils", "xz", "xz", "xz"),
    ("", "bzip2", "bzip2", "bzip2", "bzip2"),
    ("", "zstd", "zstd", "zstd", "zstd"),
    ("", "unzip", "unzip", "unzip", "unzip"),
    ("", "zip", "zip", "zip", "zip"),
    ("", "curl", "curl", "curl", "curl"),
    ("", "wget", "wget", "wget", "wget"),
    ("", "openssl", "openssl", "openssl", "openssl"),
    ("", "ca-certificates", "ca-certificates", "ca-certificates", "ca-certificates"),
    ("", "gnupg", "gnupg2", "gpg2", "gnupg"),
    ("", "python3", "python3", "python3", "python"),
    ("", "python3-gi", "python3-gobject", "python3-gobject", "python-gobject"),
    ("", "python3-dbus", "python3-dbus", "python3-dbus-python", "python-dbus"),
    ("", "python3-requests", "python3-requests", "python3-requests", "python-requests"),
    ("", "python3-yaml", "python3-pyyaml", "python3-PyYAML", "python-yaml"),
    ("", "python3-pil", "python3-pillow", "python3-Pillow", "python-pillow"),
    ("", "python3-numpy", "python3-numpy", "python3-numpy", "python-numpy"),
    ("", "python3-tk", "python3-tkinter", "python3-tk", "tk"),
    ("", "python3-cairo", "python3-cairo", "python3-cairo", "python-cairo"),
    ("", "python3-xdg", "python3-pyxdg", "python3-pyxdg", "python-pyxdg"),
    ("", "perl", "perl", "perl", "perl"),
    ("", "nodejs", "nodejs", "nodejs", "nodejs"),
    ("", "default-jre", "java-21-openjdk", "java-21-openjdk", "jre-openjdk"),
    ("", "xdg-utils", "xdg-utils", "xdg-utils", "xdg-utils"),
    ("", "desktop-file-utils", "desktop-file-utils", "desktop-file-utils", "desktop-file-utils"),
    ("", "hicolor-icon-theme", "hicolor-icon-theme", "hicolor-icon-theme", "hicolor-icon-theme"),
    ("", "adwaita-icon-theme", "adwaita-icon-theme", "adwaita-icon-theme", "adwaita-icon-theme"),
    ("", "shared-mime-info", "shared-mime-info", "shared-mime-info", "shared-mime-info"),
    ("", "gsettings-desktop-schemas", "gsettings-desktop-schemas", "gsettings-desktop-schemas", "gsettings-desktop-schemas"),
    ("", "libglib2.0-bin", "glib2", "glib2-tools", "glib2"),
    ("", "xdg-desktop-portal", "xdg-desktop-portal", "xdg-desktop-portal", "xdg-desktop-portal"),
    ("", "policykit-1", "polkit", "polkit", "polkit"),
    ("", "pkexec", "polkit", "polkit", "polkit"),
    ("", "dbus", "dbus", "dbus-1", "dbus"),
    ("", "dbus-x11", "dbus-x11", "dbus-1-x11", "dbus"),
    ("", "systemd", "systemd", "systemd", "systemd"),
    ("", "procps", "procps-ng", "procps", "procps-ng"),
    ("", "psmisc", "psmisc", "psmisc", "psmisc"),
    ("", "lsb-release", "lsb_release", "lsb-release", "lsb-release"),
    ("", "zenity", "zenity", "zenity", "zenity"),
    ("", "kdialog", "kdialog", "kdialog", "kdialog"),
    ("", "libnotify-bin", "libnotify", "libnotify-tools", "libnotify"),
    ("", "gnome-keyring", "gnome-keyring", "gnome-keyring", "gnome-keyring"),
    ("", "fonts-liberation", "liberation-fonts", "liberation-fonts", "ttf-liberation"),
    ("", "fonts-liberation2", "liberation-fonts", "liberation-fonts", "ttf-liberation"),
    ("", "fonts-dejavu-core", "dejavu-sans-fonts", "dejavu-fonts", "ttf-dejavu"),
    ("", "fonts-noto-color-emoji", "google-noto-color-emoji-fonts", "google-noto-coloremoji-fonts", "noto-fonts-emoji"),
    ("", "libnss3-tools", "nss-tools", "mozilla-nss-tools", "nss"),
    ("", "libfuse2", "fuse-libs", "libfuse2", "fuse2"),
    ("", "fuse3", "fuse3", "fuse3", "fuse3"),
    ("", "ffmpeg", "ffmpeg-free", "ffmpeg", "ffmpeg"),
    ("", "pciutils", "pciutils", "pciutils", "pciutils"),
    ("", "usbutils", "usbutils", "usbutils", "usbutils"),
    ("", "kmod", "kmod", "kmod", "kmod"),
    ("", "iproute2", "iproute", "iproute2", "iproute2"),
    ("", "net-tools", "net-tools", "net-tools", "net-tools"),
    ("", "iptables", "iptables", "iptables", "iptables"),
    ("", "sudo", "sudo", "sudo", "sudo"),
    ("", "file", "file", "file", "file"),
    ("", "tzdata", "tzdata", "timezone", "tzdata"),
    ("", "git", "git", "git", "git"),
    ("", "make", "make", "make", "make"),
    ("", "gcc", "gcc", "gcc", "gcc"),
    ("", "pulseaudio", "pulseaudio", "pulseaudio", "pulseaudio"),
    ("", "pipewire", "pipewire", "pipewire", "pipewire"),
    ("", "libgtk-3-bin", "gtk3", "gtk3-tools", "gtk3"),
    ("", "xbitmaps", "xorg-x11-xbitmaps", "xbitmaps", "xbitmaps"),
    ("", "x11-common", "", "", ""),
    ("", "xserver-xorg-core", "xorg-x11-server-Xorg", "xorg-x11-server", "xorg-server"),
    ("", "x11-xserver-utils", "xorg-x11-server-utils", "xorg-x11-server-extra", "xorg-xrandr"),
];

/// Paquets propres à une famille, inutiles (ou impossibles) ailleurs : on les ignore.
pub fn is_family_specific(name: &str) -> bool {
    const SKIP: &[&str] = &[
        "dpkg", "debconf", "debconf-2.0", "cdebconf", "init-system-helpers", "lsb-base", "sysvinit-utils", "libc-bin",
        "dpkg-dev", "ucf", "install-info", "base-files", "base-passwd", "apt", "adduser", "passwd", "sensible-utils",
        "mime-support", "media-types", "misc:depends", "shlibs:depends", "chkconfig", "systemd-units",
        "redhat-rpm-config", "rpm", "filesystem", "setup", "pacman", "glibc-common", "glibc-minimal-langpack",
        "shadow-utils", "update-alternatives", "alternatives", "/bin/sh", "/usr/bin/sh", "/usr/bin/env", "/bin/env",
    ];
    let n = name.to_ascii_lowercase();
    SKIP.contains(&n.as_str()) || n.starts_with("rpmlib(") || n.starts_with("config(") || n.starts_with("rtld(")
        || n.starts_with("pkgconfig(") || n.starts_with("debuginfo(") || n.starts_with("${")
}

fn col(row: &Row, f: Family) -> &'static str {
    match f {
        Family::Debian => row.1,
        Family::Fedora => row.2,
        Family::Suse => row.3,
        Family::Arch => row.4,
        Family::Generic => "",
    }
}

/// Nom du paquet qui fournit une bibliothèque, pour une famille donnée.
pub fn package_for_soname(soname: &str, f: Family) -> Option<&'static str> {
    TABLE.iter().find(|r| !r.0.is_empty() && r.0 == soname).map(|r| col(r, f)).filter(|s| !s.is_empty())
}

/// Bibliothèques (sonames) fournies par un paquet connu de la table.
pub fn sonames_for_package(name: &str, from: Family) -> Vec<&'static str> {
    let fams: &[Family] = match from {
        Family::Fedora | Family::Suse => &[Family::Fedora, Family::Suse],
        Family::Debian => &[Family::Debian],
        Family::Arch => &[Family::Arch],
        Family::Generic => &[],
    };
    TABLE
        .iter()
        .filter(|r| !r.0.is_empty() && fams.iter().any(|f| col(r, *f) == name))
        .map(|r| r.0)
        .collect()
}

/// Équivalent d'un nom de paquet d'une famille vers une autre.
pub fn translate(name: &str, from: Family, to: Family) -> Option<&'static str> {
    if from == to {
        return None;
    }
    // Pour les colonnes rpm, Fedora et openSUSE partagent souvent les mêmes noms.
    let froms: &[Family] = match from {
        Family::Fedora => &[Family::Fedora, Family::Suse],
        Family::Suse => &[Family::Suse, Family::Fedora],
        f => std::slice::from_ref(match f {
            Family::Debian => &Family::Debian,
            Family::Arch => &Family::Arch,
            _ => &Family::Generic,
        }),
    };
    for f in froms {
        if let Some(r) = TABLE.iter().find(|r| col(r, *f) == name) {
            let t = col(r, to);
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

/// Suggestions pour l'installateur .sh : « apt:x dnf:y zypper:z pacman:w ».
pub fn hint_line(soname: &str) -> Option<String> {
    let r = TABLE.iter().find(|r| r.0 == soname)?;
    let mut parts = Vec::new();
    for (k, v) in [("apt", r.1), ("dnf", r.2), ("zypper", r.3), ("pacman", r.4)] {
        if !v.is_empty() {
            parts.push(format!("{k}:{v}"));
        }
    }
    Some(parts.join(" "))
}

/// Nom Debian déduit des règles de nommage (« libfoo.so.3 » → « libfoo3 »).
pub fn debian_convention(soname: &str) -> Option<String> {
    let (base, ver) = soname.split_once(".so")?;
    let ver = ver.trim_start_matches('.');
    let base = base.to_ascii_lowercase().replace('_', "-");
    if ver.is_empty() {
        return Some(base);
    }
    let ver_main = ver.split('.').next().unwrap_or(ver);
    if base.ends_with(|c: char| c.is_ascii_digit()) {
        Some(format!("{base}-{ver_main}"))
    } else {
        Some(format!("{base}{ver_main}"))
    }
}

/// Paquet correspondant à une commande requise par chemin (« /usr/bin/python3 »).
pub fn package_for_file(path: &str, to: Family) -> Option<&'static str> {
    let cmd = path.rsplit('/').next().unwrap_or(path);
    let deb = match cmd {
        "python3" | "python" => "python3",
        "perl" => "perl",
        "bash" => "bash",
        "node" | "nodejs" => "nodejs",
        "zenity" => "zenity",
        "xdg-open" | "xdg-mime" | "xdg-settings" => "xdg-utils",
        "update-desktop-database" => "desktop-file-utils",
        "gtk-update-icon-cache" => "libgtk-3-bin",
        "curl" => "curl",
        "wget" => "wget",
        "gpg" | "gpg2" => "gnupg",
        _ => return None,
    };
    let row = TABLE.iter().find(|r| r.0.is_empty() && r.1 == deb)?;
    let v = col(row, to);
    if v.is_empty() { None } else { Some(v) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookups() {
        assert_eq!(package_for_soname("libgtk-3.so.0", Family::Arch), Some("gtk3"));
        assert_eq!(translate("libgtk-3-0", Family::Debian, Family::Fedora), Some("gtk3"));
        assert_eq!(translate("gtk3", Family::Fedora, Family::Debian), Some("libgtk-3-0"));
        assert_eq!(translate("python3-gi", Family::Debian, Family::Arch), Some("python-gobject"));
        assert_eq!(debian_convention("libgtk-3.so.0").as_deref(), Some("libgtk-3-0"));
        assert_eq!(debian_convention("libssl.so.3").as_deref(), Some("libssl3"));
        assert_eq!(debian_convention("libX11.so.6").as_deref(), Some("libx11-6"));
        assert_eq!(debian_convention("libgcc_s.so.1").as_deref(), Some("libgcc-s1"));
        assert!(is_family_specific("debconf"));
        assert!(is_family_specific("rpmlib(CompressedFileNames)"));
        assert_eq!(package_for_file("/usr/bin/python3", Family::Arch), Some("python"));
        assert!(hint_line("libgtk-3.so.0").unwrap().contains("dnf:gtk3"));
    }
}
