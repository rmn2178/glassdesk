import en from "../../../apps/settings/src/i18n/en.json";
import es from "../../../apps/settings/src/i18n/es.json";
import fr from "../../../apps/settings/src/i18n/fr.json";
import zh from "../../../apps/settings/src/i18n/zh.json";
import hi from "../../../apps/settings/src/i18n/hi.json";
import ar from "../../../apps/settings/src/i18n/ar.json";
import bn from "../../../apps/settings/src/i18n/bn.json";
import pt from "../../../apps/settings/src/i18n/pt.json";
import { site } from "./site";

export const settingsLanguages = [
  { value: "en", label: "English" },
  { value: "es", label: "Español" },
  { value: "fr", label: "Français" },
  { value: "zh", label: "简体中文" },
  { value: "hi", label: "हिन्दी" },
  { value: "ar", label: "العربية" },
  { value: "bn", label: "বাংলা" },
  { value: "pt", label: "Português" },
] as const;

/** Only demo-specific text lives here; app labels come from the real catalogs. */
function preview(t: typeof en, title: string, fixedItemsHint: string, accessoryPreviewHint: string) {
  return {
    title, fixedItemsHint, accessoryPreviewHint,
    dock: t.dock, windows: t.windows, menubar: t.menuBar, system: t.system,
    size: t.size, dockDensity: t.dockDensity, compact: t.dockDensityCompact,
    normal: t.dockDensityNormal, spacious: t.dockDensitySpacious,
    dockAutoHideSpeed: t.dockAutoHideSpeed, smooth: t.dockAutoHideSmooth,
    fast: t.genieSpeedFast, fixedItems: t.dockItems, showStart: t.showStart,
    showDownloads: t.showDownloads, showTrash: t.showTrash, pinExplorer: t.pinExplorer,
    pinExplorerHint: t.pinExplorerHint, small: t.small, large: t.large,
    magnification: t.magnification, magnificationHint: t.magnificationHint,
    magnifiedSize: t.magnifiedSize, autoHide: t.menuBarAutoHide,
    autoHideHint: t.dockHidingAutomaticHint, menuBar: t.menuBarShow,
    menuBarHint: t.menuBarHint, menuBarAutoHide: t.menuBarAutoHide,
    menuBarAutoHideHint: t.menuBarAutoHideHint, aiUsage: t.aiUsage,
    aiUsageHint: t.aiUsageHint, accessoryBattery: t.accessoryBattery,
    accessoryBatteryHint: t.accessoryBatteryHint, genie: t.genie, genieHint: t.genieHint,
    showMinimized: t.showMinimized, showMinimizedHint: t.showMinimizedHint,
    autostart: t.autostart, mascot: t.mascot, mascotHint: t.mascotHint,
    autoUpdate: t.autoUpdate, autoUpdateHint: t.autoUpdateHint,
    language: t.language, languageAuto: t.languageAuto, updates: t.updates,
    updatesCurrent: t.updatesCurrent.replace("{version}", site.version),
    updatesChecking: t.updatesChecking, updatesCheck: t.updatesCheck,
  };
}

export const settingsText = {
  en: preview(en, "Nosih Settings",
    "You can also right-click Start, Downloads or Recycle Bin in Nosih and choose Remove from Dock.",
    "Preview values only; device support varies."),
  es: preview(es, "Ajustes de Nosih",
    "También puedes hacer clic derecho en Inicio, Descargas o Papelera y elegir Quitar del Dock.",
    "Valores de ejemplo; la compatibilidad depende del dispositivo."),
  fr: preview(fr, "Réglages de Nosih",
    "Vous pouvez aussi retirer Démarrer, Téléchargements ou la Corbeille par un clic droit dans Nosih.",
    "Valeurs de démonstration ; la compatibilité dépend de l’appareil."),
  zh: preview(zh, "Nosih 设置",
    "也可在 Nosih 中右键点击开始菜单、下载或回收站，将其从程序坞中移除。",
    "仅为演示数值，兼容性因设备而异。"),
  hi: preview(hi, "Nosih सेटिंग्स",
    "Nosih में स्टार्ट, डाउनलोड या रीसायकल बिन पर दायाँ क्लिक करके भी उसे डॉक से हटा सकते हैं।",
    "ये उदाहरण के मान हैं; अनुकूलता डिवाइस पर निर्भर करती है।"),
  ar: preview(ar, "إعدادات Nosih",
    "يمكنك أيضًا النقر بزر الفأرة الأيمن على ابدأ أو التنزيلات أو سلة المحذوفات لإزالتها من Dock.",
    "قيم توضيحية فقط؛ يختلف الدعم حسب الجهاز."),
  bn: preview(bn, "Nosih সেটিংস",
    "Nosih-এ স্টার্ট, ডাউনলোড বা রিসাইকেল বিনে ডান ক্লিক করেও ডক থেকে সরাতে পারেন।",
    "এগুলো উদাহরণের মান; সামঞ্জস্য ডিভাইসের ওপর নির্ভর করে।"),
  pt: preview(pt, "Configurações do Nosih",
    "Você também pode clicar com o botão direito em Iniciar, Downloads ou Lixeira para remover do Dock.",
    "Valores de exemplo; a compatibilidade depende do dispositivo."),
};

export type SettingsKey = keyof (typeof settingsText)["en"];
