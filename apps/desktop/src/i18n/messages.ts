// UI copy catalogue. English is the reference; every locale must define every key
// (enforced by the `Messages` type). Keep strings short, plain and status-first (design.md §9).
export const en = {
  "app.name": "Downova",
  "app.skipToContent": "Skip to download list",
  "app.version": "Version 0.1.0 — development foundation build",

  "unavailable.service":
    "Unavailable: the background download service is not implemented or connected yet.",
  "unavailable.notWired":
    "Unavailable: the download service is running, but this command is connected to it in a later milestone.",
  "unavailable.selection": "Unavailable: there are no downloads to select yet.",
  "unavailable.updates":
    "Unavailable: signed updates are not set up yet (planned for the public beta).",

  "menu.label": "Application menu",
  "menu.tasks": "Tasks",
  "menu.file": "File",
  "menu.downloads": "Downloads",
  "menu.view": "View",
  "menu.help": "Help",
  "menu.tasks.addUrl": "Add new download…",
  "menu.tasks.addBatch": "Add batch download from clipboard",
  "menu.tasks.import": "Import download list…",
  "menu.tasks.exit": "Exit and stop downloads",
  "menu.file.resume": "Resume download",
  "menu.file.stop": "Stop download",
  "menu.file.remove": "Remove from list",
  "menu.file.redownload": "Redownload",
  "menu.file.properties": "Properties",
  "menu.downloads.stopAll": "Stop all",
  "menu.downloads.deleteCompleted": "Delete all completed",
  "menu.downloads.scheduler": "Scheduler…",
  "menu.downloads.startQueue": "Start queue",
  "menu.downloads.stopQueue": "Stop queue",
  "menu.downloads.options": "Options…",
  "menu.downloads.speedLimiter": "Speed limiter…",
  "menu.view.categories": "Show categories",
  "menu.view.themeSystem": "Theme: match system",
  "menu.view.themeLight": "Theme: light",
  "menu.view.themeDark": "Theme: dark",
  "menu.view.langEn": "English",
  "menu.view.langBn": "বাংলা (Bangla)",
  "menu.help.about": "About Downova",
  "menu.help.updates": "Check for updates…",

  "toolbar.label": "Download commands",
  "toolbar.addUrl": "Add URL",
  "toolbar.resume": "Resume",
  "toolbar.stop": "Stop",
  "toolbar.stopAll": "Stop all",
  "toolbar.delete": "Delete",
  "toolbar.deleteCompleted": "Delete completed",
  "toolbar.options": "Options",
  "toolbar.scheduler": "Scheduler",
  "toolbar.startQueue": "Start queue",
  "toolbar.stopQueue": "Stop queue",

  "categories.label": "Categories",
  "categories.all": "All downloads",
  "categories.compressed": "Compressed",
  "categories.documents": "Documents",
  "categories.music": "Music",
  "categories.programs": "Programs",
  "categories.video": "Video",
  "categories.unfinished": "Unfinished",
  "categories.finished": "Finished",
  "categories.queues": "Queues",
  "categories.mainQueue": "Main download queue",
  "categories.expand": "Expand",
  "categories.collapse": "Collapse",
  "categories.hide": "Hide categories",

  "table.caption": "Downloads in {category}",
  "table.fileName": "File name",
  "table.size": "Size",
  "table.status": "Status",
  "table.timeLeft": "Time left",
  "table.rate": "Transfer rate",
  "table.lastTry": "Last try",
  "table.description": "Description",
  "table.empty": "No downloads in this category.",
  "table.emptyDisconnected":
    "The download list comes from the background service, which is not connected in this build.",
  "table.unknownSize": "Unknown",

  "status.queued": "Queued",
  "status.probing": "Connecting",
  "status.transferring": "Downloading {percent}",
  "status.verifying": "Verifying",
  "status.scanning": "Scanning",
  "status.finalizing": "Finishing",
  "status.completed": "Complete",
  "status.failed": "Error",
  "status.cancelled": "Cancelled",
  "status.paused": "Paused",
  "status.waiting": "Waiting: {reason}",
  "blocker.offline": "offline",
  "blocker.metered_network": "metered network",
  "blocker.schedule_window": "schedule",
  "blocker.rate_limit_cooldown": "server asked to wait",
  "blocker.disk_unavailable": "disk unavailable",
  "blocker.volume_missing": "drive disconnected",
  "blocker.queue_limit": "queue limit",

  // Error catalogue (CORE-11), keyed by the contract's ErrorKind and NextAction wire names.
  // Clients never parse the service's English `detail`; these strings say what the user can do.
  "error.kind.NETWORK_OFFLINE": "The connection to the server failed or was interrupted.",
  "error.kind.AUTH_REQUIRED": "The server or a proxy requires a sign-in.",
  "error.kind.LINK_REFRESH_REQUIRED":
    "The server refused the link or no longer has the file. The link may have expired, been revoked or need a sign-in.",
  "error.kind.REMOTE_CHANGED":
    "The file on the server changed after the download started, so the saved part cannot be used.",
  "error.kind.RANGE_UNSUPPORTED":
    "The server cannot send part of a file, so the download cannot continue where it stopped.",
  "error.kind.INVALID_RANGE":
    "The server sent data that does not fit what was asked for, or the file has the wrong length. It was not used.",
  "error.kind.CHECKSUM_MISMATCH":
    "The downloaded file does not match the expected checksum. It was deleted, not saved.",
  "error.kind.DISK_FULL": "The destination drive does not have enough free space.",
  "error.kind.VOLUME_MISSING": "The destination drive or folder is not available.",
  "error.kind.DESTINATION_UNWRITABLE":
    "The file cannot be saved in the download folder: the name is not allowed there, there is no permission, the drive is read-only, or another file already has the name.",
  "error.kind.HOST_NOT_INSTALLED": "The browser cannot reach Downova's helper program.",
  "error.kind.SCAN_BLOCKED": "A security scan blocked this file.",
  "error.kind.BACKEND_CAPABILITY_UNAVAILABLE": "The download engine cannot do this safely.",
  "error.kind.SERVER_BUSY": "The server is busy or is limiting requests.",
  "error.kind.TLS_HANDSHAKE_FAILED":
    "A secure connection could not be made: the site's certificate is not trusted, has expired or does not match the site.",
  "error.kind.SERVICE_UNAVAILABLE":
    "The Downova background service is not running or did not respond.",
  "error.kind.INVALID_REQUEST": "The request or the server's response could not be accepted.",
  "error.kind.IDEMPOTENCY_CONFLICT": "This request was already used for a different download.",
  "error.kind.INTERNAL": "Downova ran into an unexpected problem.",
  "error.action.retry": "Try again in a moment.",
  "error.action.wait": "Wait a while before trying again.",
  "error.action.refresh_link": "Get a new link from the website and add it.",
  "error.action.sign_in":
    "Downova cannot sign in or use browser cookies yet. Download this file in your browser instead.",
  "error.action.free_disk_space": "Free up space on the destination drive.",
  "error.action.reconnect_volume": "Reconnect the drive or restore the download folder.",
  "error.action.install_desktop_app": "Install the Downova desktop app.",
  "error.action.choose_restart_or_keep":
    "The saved part cannot be continued safely, so the file has to be downloaded again from the start.",
  "error.action.contact_support": "Please report this problem to the Downova developers.",
  "error.followUp.failed":
    "This download has stopped. Adding its link again starts it over from the beginning.",
  "error.followUp.continues": "The download continues by itself once this is fixed.",
  "error.followUp.autoRetry": "Downova will try again automatically.",

  "statusbar.label": "Status",
  "statusbar.checking": "Checking the download service…",
  "statusbar.disconnected": "Download service not connected",
  "statusbar.connected": "Connected to download service",
  "statusbar.count": "{count} downloads",

  "options.title": "Options",
  "options.close": "Close",
  "options.done": "Done",
  "options.language": "Language",
  "options.theme": "Theme",
  "options.theme.system": "Match system",
  "options.theme.light": "Light",
  "options.theme.dark": "Dark",
  "options.localNote":
    "These preferences are stored only in this window's local storage until the settings service exists.",
  "language.en": "English",
  "language.bn": "বাংলা (Bangla)",

  "about.title": "About Downova",
  "about.body":
    "A free, account-free download manager. This build is a development foundation: the background service and download engine are not implemented yet, so nothing can be downloaded.",
} as const;

export type MessageKey = keyof typeof en;
export type Messages = Record<MessageKey, string>;

export const bn: Messages = {
  "app.name": "Downova",
  "app.skipToContent": "ডাউনলোড তালিকায় যান",
  "app.version": "সংস্করণ 0.1.0 — development-এর ভিত্তি build",

  "unavailable.service":
    "এখন ব্যবহার করা যাবে না: ব্যাকগ্রাউন্ড ডাউনলোড সার্ভিস এখনো তৈরি বা যুক্ত হয়নি।",
  "unavailable.notWired":
    "এখন ব্যবহার করা যাবে না: ডাউনলোড সার্ভিস চলছে, কিন্তু এই কমান্ডটি পরের ধাপে সার্ভিসের সাথে যুক্ত হবে।",
  "unavailable.selection": "এখন ব্যবহার করা যাবে না: নির্বাচন করার মতো কোনো ডাউনলোড এখনো নেই।",
  "unavailable.updates":
    "এখন ব্যবহার করা যাবে না: স্বাক্ষরিত আপডেট এখনো চালু হয়নি (public beta-তে আসবে)।",

  "menu.label": "অ্যাপ্লিকেশন মেনু",
  "menu.tasks": "কাজ",
  "menu.file": "ফাইল",
  "menu.downloads": "ডাউনলোড",
  "menu.view": "দেখুন",
  "menu.help": "সাহায্য",
  "menu.tasks.addUrl": "নতুন ডাউনলোড যোগ করুন…",
  "menu.tasks.addBatch": "ক্লিপবোর্ড থেকে একাধিক ডাউনলোড যোগ করুন",
  "menu.tasks.import": "ডাউনলোড তালিকা আমদানি করুন…",
  "menu.tasks.exit": "বন্ধ করুন ও ডাউনলোড থামান",
  "menu.file.resume": "ডাউনলোড চালু করুন",
  "menu.file.stop": "ডাউনলোড থামান",
  "menu.file.remove": "তালিকা থেকে সরান",
  "menu.file.redownload": "আবার ডাউনলোড করুন",
  "menu.file.properties": "বৈশিষ্ট্য",
  "menu.downloads.stopAll": "সব থামান",
  "menu.downloads.deleteCompleted": "সম্পন্ন সব মুছুন",
  "menu.downloads.scheduler": "সময়সূচি…",
  "menu.downloads.startQueue": "কিউ শুরু করুন",
  "menu.downloads.stopQueue": "কিউ থামান",
  "menu.downloads.options": "অপশন…",
  "menu.downloads.speedLimiter": "গতি সীমা…",
  "menu.view.categories": "ক্যাটাগরি দেখান",
  "menu.view.themeSystem": "থিম: সিস্টেমের মতো",
  "menu.view.themeLight": "থিম: হালকা",
  "menu.view.themeDark": "থিম: গাঢ়",
  "menu.view.langEn": "English",
  "menu.view.langBn": "বাংলা (Bangla)",
  "menu.help.about": "Downova সম্পর্কে",
  "menu.help.updates": "আপডেট খুঁজুন…",

  "toolbar.label": "ডাউনলোড কমান্ড",
  "toolbar.addUrl": "URL যোগ",
  "toolbar.resume": "চালু",
  "toolbar.stop": "থামান",
  "toolbar.stopAll": "সব থামান",
  "toolbar.delete": "মুছুন",
  "toolbar.deleteCompleted": "সম্পন্ন মুছুন",
  "toolbar.options": "অপশন",
  "toolbar.scheduler": "সময়সূচি",
  "toolbar.startQueue": "কিউ শুরু",
  "toolbar.stopQueue": "কিউ থামান",

  "categories.label": "ক্যাটাগরি",
  "categories.all": "সব ডাউনলোড",
  "categories.compressed": "সংকুচিত",
  "categories.documents": "ডকুমেন্ট",
  "categories.music": "গান",
  "categories.programs": "প্রোগ্রাম",
  "categories.video": "ভিডিও",
  "categories.unfinished": "অসম্পূর্ণ",
  "categories.finished": "সম্পূর্ণ",
  "categories.queues": "কিউ",
  "categories.mainQueue": "প্রধান ডাউনলোড কিউ",
  "categories.expand": "খুলুন",
  "categories.collapse": "বন্ধ করুন",
  "categories.hide": "ক্যাটাগরি লুকান",

  "table.caption": "{category}-এর ডাউনলোড",
  "table.fileName": "ফাইলের নাম",
  "table.size": "আকার",
  "table.status": "অবস্থা",
  "table.timeLeft": "বাকি সময়",
  "table.rate": "গতি",
  "table.lastTry": "শেষ চেষ্টা",
  "table.description": "বিবরণ",
  "table.empty": "এই ক্যাটাগরিতে কোনো ডাউনলোড নেই।",
  "table.emptyDisconnected":
    "ডাউনলোডের তালিকা ব্যাকগ্রাউন্ড সার্ভিস থেকে আসে, যা এই build-এ যুক্ত নয়।",
  "table.unknownSize": "অজানা",

  "status.queued": "অপেক্ষমাণ",
  "status.probing": "সংযোগ হচ্ছে",
  "status.transferring": "ডাউনলোড হচ্ছে {percent}",
  "status.verifying": "যাচাই হচ্ছে",
  "status.scanning": "স্ক্যান হচ্ছে",
  "status.finalizing": "শেষ করা হচ্ছে",
  "status.completed": "সম্পূর্ণ",
  "status.failed": "ত্রুটি",
  "status.cancelled": "বাতিল",
  "status.paused": "থামানো",
  "status.waiting": "অপেক্ষা: {reason}",
  "blocker.offline": "ইন্টারনেট নেই",
  "blocker.metered_network": "মিটারড নেটওয়ার্ক",
  "blocker.schedule_window": "সময়সূচি",
  "blocker.rate_limit_cooldown": "সার্ভার অপেক্ষা করতে বলেছে",
  "blocker.disk_unavailable": "ডিস্ক পাওয়া যাচ্ছে না",
  "blocker.volume_missing": "ড্রাইভ বিচ্ছিন্ন",
  "blocker.queue_limit": "কিউ সীমা",

  "error.kind.NETWORK_OFFLINE": "সার্ভারের সাথে সংযোগ ব্যর্থ হয়েছে বা মাঝপথে বিচ্ছিন্ন হয়েছে।",
  "error.kind.AUTH_REQUIRED": "সার্ভার বা প্রক্সি সাইন-ইন চাইছে।",
  "error.kind.LINK_REFRESH_REQUIRED":
    "সার্ভার লিংকটি গ্রহণ করেনি বা ফাইলটি সেখানে আর নেই। লিংকের মেয়াদ শেষ হয়ে থাকতে পারে, লিংকটি বাতিল হয়ে থাকতে পারে, বা সাইন-ইন লাগতে পারে।",
  "error.kind.REMOTE_CHANGED":
    "ডাউনলোড শুরুর পর সার্ভারের ফাইলটি বদলে গেছে, তাই সংরক্ষিত অংশটি ব্যবহার করা যাবে না।",
  "error.kind.RANGE_UNSUPPORTED":
    "সার্ভার ফাইলের আংশিক অংশ পাঠাতে পারে না, তাই যেখানে থেমেছিল সেখান থেকে ডাউনলোড চালিয়ে নেওয়া যাবে না।",
  "error.kind.INVALID_RANGE":
    "সার্ভারের পাঠানো ডেটা চাওয়া অংশের সাথে মেলে না, বা ফাইলের দৈর্ঘ্য ভুল। এই ডেটা ব্যবহার করা হয়নি।",
  "error.kind.CHECKSUM_MISMATCH":
    "ডাউনলোড করা ফাইলটি প্রত্যাশিত checksum-এর সাথে মেলেনি। ফাইলটি সংরক্ষণ না করে মুছে ফেলা হয়েছে।",
  "error.kind.DISK_FULL": "গন্তব্য ড্রাইভে যথেষ্ট খালি জায়গা নেই।",
  "error.kind.VOLUME_MISSING": "গন্তব্য ড্রাইভ বা ফোল্ডার পাওয়া যাচ্ছে না।",
  "error.kind.DESTINATION_UNWRITABLE":
    "ডাউনলোড ফোল্ডারে ফাইলটি সংরক্ষণ করা যাচ্ছে না: নামটি সেখানে গ্রহণযোগ্য নয়, অনুমতি নেই, ড্রাইভটি শুধু পড়ার জন্য (read-only), অথবা এই নামে আরেকটি ফাইল আগে থেকেই আছে।",
  "error.kind.HOST_NOT_INSTALLED":
    "ব্রাউজার Downova-র সহায়ক প্রোগ্রামের সাথে যোগাযোগ করতে পারছে না।",
  "error.kind.SCAN_BLOCKED": "নিরাপত্তা স্ক্যান এই ফাইলটি আটকে দিয়েছে।",
  "error.kind.BACKEND_CAPABILITY_UNAVAILABLE": "ডাউনলোড ইঞ্জিন এটি নিরাপদে করতে পারে না।",
  "error.kind.SERVER_BUSY": "সার্ভার ব্যস্ত, অথবা অনুরোধের সংখ্যা সীমিত করছে।",
  "error.kind.TLS_HANDSHAKE_FAILED":
    "নিরাপদ সংযোগ তৈরি করা যায়নি: সাইটের সার্টিফিকেট বিশ্বস্ত নয়, মেয়াদোত্তীর্ণ, অথবা সাইটের সাথে মেলে না।",
  "error.kind.SERVICE_UNAVAILABLE": "Downova-র ব্যাকগ্রাউন্ড সার্ভিস চলছে না বা সাড়া দেয়নি।",
  "error.kind.INVALID_REQUEST": "অনুরোধটি বা সার্ভারের উত্তরটি গ্রহণ করা যায়নি।",
  "error.kind.IDEMPOTENCY_CONFLICT":
    "এই অনুরোধটি আগেই অন্য একটি ডাউনলোডের জন্য ব্যবহার করা হয়েছে।",
  "error.kind.INTERNAL": "Downova একটি অপ্রত্যাশিত সমস্যায় পড়েছে।",
  "error.action.retry": "একটু পরে আবার চেষ্টা করুন।",
  "error.action.wait": "কিছুক্ষণ অপেক্ষা করে আবার চেষ্টা করুন।",
  "error.action.refresh_link": "ওয়েবসাইট থেকে নতুন লিংক নিয়ে সেটি যোগ করুন।",
  "error.action.sign_in":
    "Downova এখনো সাইন-ইন করতে বা ব্রাউজারের কুকি ব্যবহার করতে পারে না। ফাইলটি বরং ব্রাউজারে ডাউনলোড করুন।",
  "error.action.free_disk_space": "গন্তব্য ড্রাইভে জায়গা খালি করুন।",
  "error.action.reconnect_volume": "ড্রাইভটি আবার যুক্ত করুন বা ডাউনলোড ফোল্ডারটি ফিরিয়ে আনুন।",
  "error.action.install_desktop_app": "Downova ডেস্কটপ অ্যাপ ইনস্টল করুন।",
  "error.action.choose_restart_or_keep":
    "সংরক্ষিত অংশটি নিরাপদে চালিয়ে নেওয়া যাবে না, তাই ফাইলটি শুরু থেকে আবার ডাউনলোড করতে হবে।",
  "error.action.contact_support": "অনুগ্রহ করে সমস্যাটি Downova-র ডেভেলপারদের জানান।",
  "error.followUp.failed":
    "এই ডাউনলোডটি থেমে গেছে। লিংকটি আবার যোগ করলে এটি শুরু থেকে নতুন করে চলবে।",
  "error.followUp.continues": "সমস্যাটি ঠিক হলে ডাউনলোড নিজে থেকেই চলবে।",
  "error.followUp.autoRetry": "Downova নিজে থেকেই আবার চেষ্টা করবে।",

  "statusbar.label": "অবস্থা",
  "statusbar.checking": "ডাউনলোড সার্ভিস যাচাই করা হচ্ছে…",
  "statusbar.disconnected": "ডাউনলোড সার্ভিস যুক্ত নয়",
  "statusbar.connected": "ডাউনলোড সার্ভিসে যুক্ত",
  "statusbar.count": "{count}টি ডাউনলোড",

  "options.title": "অপশন",
  "options.close": "বন্ধ করুন",
  "options.done": "ঠিক আছে",
  "options.language": "ভাষা",
  "options.theme": "থিম",
  "options.theme.system": "সিস্টেমের মতো",
  "options.theme.light": "হালকা",
  "options.theme.dark": "গাঢ়",
  "options.localNote":
    "সেটিংস সার্ভিস তৈরি না হওয়া পর্যন্ত এই পছন্দগুলো শুধু এই উইন্ডোর local storage-এ থাকে।",
  "language.en": "English",
  "language.bn": "বাংলা (Bangla)",

  "about.title": "Downova সম্পর্কে",
  "about.body":
    "বিনামূল্যের, অ্যাকাউন্ট ছাড়া ব্যবহারযোগ্য ডাউনলোড ম্যানেজার। এটি development-এর ভিত্তি build: ব্যাকগ্রাউন্ড সার্ভিস ও ডাউনলোড ইঞ্জিন এখনো তৈরি হয়নি, তাই কিছুই ডাউনলোড করা যাবে না।",
};

export const catalogs = { en, bn } satisfies Record<string, Messages>;
export type Locale = keyof typeof catalogs;
export const LOCALES = Object.keys(catalogs) as Locale[];

/** Replaces `{name}` placeholders. Values are inserted as plain text. */
export function format(template: string, values: Record<string, string | number>): string {
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in values ? String(values[name]) : match,
  );
}
