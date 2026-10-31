// English/Bangla copy of the extension (B2.13, B2.20): the options page, the popup (hand-offs,
// service readiness, capture summary), capture outcomes and the context menu. This is the one
// catalogue; records store stable codes, never translated text. The language follows the browser UI (`chrome.i18n.getUILanguage`).
import type { BrowserSide, CaptureState } from "./captures.js";
import type { Reason } from "./policy.js";
import type { HandoffCode, HandoffState, ServiceCode } from "../handoff.js";

const en = {
  title: "Downova — automatic capture",
  intro:
    "Downova can take over ordinary downloads from sites you choose. It is off by default. The link you right-click with “Download with Downova” works either way.",
  enable: "Capture downloads automatically",
  enableHelp:
    "When you turn this on, the browser asks to let Downova see your downloads and the requests behind them (to check they are plain public downloads). Nothing is sent anywhere except to Downova on this computer.",
  permissionsGranted: "Permissions: granted.",
  permissionsMissing: "Permissions: not granted. Automatic capture stays off.",
  serviceChecking: "Downova: checking…",
  serviceReady: "Downova is running.",
  serviceDown: "Downova is not available. Downloads stay in the browser.",
  supportedTitle: "What is captured",
  supported1: "Only downloads from the sites below, after the browser starts them.",
  supported2:
    "Only plain public downloads: a single GET request, no cookies or sign-in sent, a normal response, and every redirect inside your sites.",
  supported3:
    "Everything else stays in the browser: form (POST) downloads, blob: and data: links, signed-in or cookie-based downloads, anything the browser warns about, programs and installers, and private windows.",
  supported4:
    "Downova cancels the browser's download only after Downova accepted it. If Downova cannot be reached or does not confirm, the browser keeps downloading.",
  sitesTitle: "Sites to capture from",
  sitesHelp: "A site includes its subdomains (example.com also covers dl.example.com).",
  siteLabel: "Site",
  addSite: "Add site",
  removeSite: "Remove {site}",
  noSites: "No sites yet. Nothing is captured until you add one.",
  excludedTitle: "Never capture from",
  excludedHelp: "Exclusions win over the sites above.",
  addExcluded: "Add exclusion",
  noExcluded: "No exclusions.",
  invalidSite: "Enter a site name such as example.com.",
  sitePermissionDenied: "The browser did not allow access to {site}. It was not added.",
  filtersTitle: "Filters",
  extensionsLabel: "File types (comma-separated, empty = any)",
  mimeLabel: "MIME types (comma-separated, empty = any)",
  minSizeLabel: "Smallest size in MB (0 = any)",
  filtersHelp:
    "A download whose type or size is unknown is not captured when a filter needs it. Programs and installers always stay in the browser.",
  saveFilters: "Save filters",
  saved: "Saved.",
  activityTitle: "Recent activity",
  noActivity: "Nothing captured yet.",
  clearActivity: "Clear activity",
  job: "Downova job {job}",
  popupOn: "Automatic capture is on for {count} site(s).",
  popupOff: "Automatic capture is off.",
  openSettings: "Automatic capture settings",
  "state.evaluating": "Checking",
  "state.browser_owned": "Kept in the browser",
  "state.sending": "Sending to Downova",
  "state.accepted": "Accepted by Downova",
  "state.uncertain": "Not confirmed",
  "state.failed": "Kept in the browser",
  "browser.running": "",
  "browser.cancelled_by_downova": "The browser's download was cancelled; Downova continues it.",
  "browser.finished_in_browser":
    "The browser finished first, so there are two copies: the browser's and Downova's.",
  "browser.cancelled_by_user": "You cancelled the browser's download; Downova's job still exists.",
  "browser.kept": "The browser's download was left as it was.",
  uncertain:
    "Downova did not confirm. The browser keeps downloading. Downova may also have added it (at most once), so you could end up with two copies.",
  "reason.unsupported_scheme":
    "This kind of link (blob:, data: or similar) can only be saved by the browser.",
  "reason.invalid_link": "The link is not an ordinary public http(s) link.",
  "reason.incognito": "Private windows are never captured.",
  "reason.site_not_allowed": "This site is not in your list.",
  "reason.site_excluded": "This site is excluded.",
  "reason.permission_missing":
    "The browser has not allowed access to this site. Add the site again.",
  "reason.browser_warning": "The browser flagged this download, so its own checks stay in charge.",
  "reason.executable": "Programs and installers always stay in the browser.",
  "reason.no_request_evidence":
    "Downova could not see the request behind this download, so it cannot know it is a plain public download.",
  "reason.ambiguous_request":
    "Several requests matched this download; Downova could not tell which one.",
  "reason.not_get": "The download came from a form or other non-GET request.",
  "reason.sent_cookies": "The browser sent cookies for this download (it may need your sign-in).",
  "reason.sent_credentials": "The browser sent sign-in credentials for this download.",
  "reason.unexpected_status": "The server's response was not an ordinary complete file.",
  "reason.redirect_not_allowed": "The download was redirected to a site outside your list.",
  "reason.mime_unknown": "The file type is unknown, and a MIME filter is set.",
  "reason.mime_filtered": "The file type does not match your MIME filter.",
  "reason.type_filtered": "The file type does not match your file-type filter.",
  "reason.size_unknown": "The size is unknown, and a minimum size is set.",
  "reason.too_small": "The file is smaller than your minimum size.",
  "reason.busy": "Downova was already taking over three downloads.",
  "reason.capture_disabled": "Automatic capture was turned off.",
  "reason.permission_revoked": "A permission was removed.",
  "reason.downova_unavailable": "Downova is not available; the browser keeps the download.",
  "reason.downova_rejected": "Downova did not accept this link; the browser keeps the download.",
  // B2.20: the explicit hand-off popup and the context menu.
  menuTitle: "Download with Downova",
  "popup.title": "Downova",
  "popup.hint":
    "Right-click a link and choose “Download with Downova”. Only public http and https links are handed off; the browser's own downloads are never changed by this.",
  "popup.serviceHeading": "Service",
  "popup.check": "Check again",
  "popup.captureHeading": "Automatic capture",
  "popup.recentHeading": "Recent hand-offs",
  "popup.loading": "Loading…",
  "popup.empty": "Nothing handed off yet.",
  "popup.clear": "Clear list",
  "service.checking": "Checking whether Downova is running…",
  "service.ready": "Service ready. Downova {version} is running (backend {backend}).",
  "service.host_missing":
    "Service unavailable. Downova's browser helper is not installed for this browser. Install or repair Downova, then check again.",
  "service.unavailable": "Service unavailable. Open the Downova app, then check again.",
  "service.no_answer":
    "Service status unknown. Downova's browser helper did not answer. Try again.",
  "service.unexpected":
    "Service status unknown. Downova's browser helper sent an unexpected answer.",
  "handoff.state.sending": "Sending",
  "handoff.state.accepted": "Accepted by Downova",
  "handoff.state.rejected": "Not accepted",
  "handoff.state.unavailable": "Downova unavailable",
  "handoff.state.uncertain": "Not confirmed",
  "handoff.code.sending": "Sending to Downova…",
  "handoff.code.accepted":
    "Downova accepted the link. That does not yet mean the transfer started or finished; follow it in the Downova app.",
  "handoff.code.rejected": "Downova did not accept this link.",
  "handoff.code.rejected_invalid": "This is not a valid link.",
  "handoff.code.rejected_scheme": "Only http and https links can be handed to Downova.",
  "handoff.code.rejected_credentials":
    "Links with an embedded user name or password stay in the browser.",
  "handoff.code.rejected_too_long": "The link is too long.",
  "handoff.code.rejected_no_host": "The link has no server name.",
  "handoff.code.rejected_spaces": "The link contains spaces or control characters.",
  "handoff.code.unavailable":
    "Downova is not available. Nothing was added; retry once Downova is running.",
  "handoff.code.host_missing":
    "Downova's browser helper is not installed for this browser. Install or repair Downova, then retry.",
  "handoff.code.uncertain":
    "No confirmation arrived. The download may or may not have been added; retry to find out (no duplicate is created).",
  "handoff.retry": "Retry",
  "handoff.retryLabel": "Retry hand-off of {link}",
  "handoff.dismiss": "Dismiss",
  "handoff.dismissLabel": "Dismiss {link}",
};

type Key = keyof typeof en;

const bn: Record<Key, string> = {
  title: "Downova — স্বয়ংক্রিয় ক্যাপচার",
  intro:
    "আপনার বেছে নেওয়া সাইটের সাধারণ ডাউনলোড Downova নিয়ে নিতে পারে। এটি শুরুতে বন্ধ থাকে। “Download with Downova” দিয়ে ডান-ক্লিক করা লিংক যেকোনো অবস্থায় কাজ করে।",
  enable: "ডাউনলোড স্বয়ংক্রিয়ভাবে ক্যাপচার করুন",
  enableHelp:
    "চালু করলে ব্রাউজার জানতে চাইবে Downova আপনার ডাউনলোড ও এর পেছনের অনুরোধ দেখতে পারবে কিনা (এগুলো সাধারণ পাবলিক ডাউনলোড কিনা যাচাই করতে)। এই কম্পিউটারের Downova ছাড়া কোথাও কিছু পাঠানো হয় না।",
  permissionsGranted: "অনুমতি: দেওয়া আছে।",
  permissionsMissing: "অনুমতি: দেওয়া হয়নি। স্বয়ংক্রিয় ক্যাপচার বন্ধ থাকে।",
  serviceChecking: "Downova: যাচাই চলছে…",
  serviceReady: "Downova চালু আছে।",
  serviceDown: "Downova পাওয়া যাচ্ছে না। ডাউনলোড ব্রাউজারেই থাকবে।",
  supportedTitle: "কী ক্যাপচার হয়",
  supported1: "শুধু নিচের সাইটগুলোর ডাউনলোড, ব্রাউজার শুরু করার পর।",
  supported2:
    "শুধু সাধারণ পাবলিক ডাউনলোড: একটি GET অনুরোধ, কোনো কুকি বা সাইন-ইন পাঠানো হয়নি, স্বাভাবিক উত্তর, এবং প্রতিটি রিডাইরেক্ট আপনার সাইটের মধ্যে।",
  supported3:
    "বাকি সব ব্রাউজারেই থাকে: ফর্ম (POST) ডাউনলোড, blob: ও data: লিংক, সাইন-ইন বা কুকি-নির্ভর ডাউনলোড, ব্রাউজার যেগুলো নিয়ে সতর্ক করে, প্রোগ্রাম ও ইনস্টলার, এবং প্রাইভেট উইন্ডো।",
  supported4:
    "Downova গ্রহণ করার পরেই শুধু ব্রাউজারের ডাউনলোড বাতিল করা হয়। Downova-কে না পেলে বা নিশ্চিত না হলে ব্রাউজার ডাউনলোড চালিয়ে যায়।",
  sitesTitle: "যেসব সাইট থেকে ক্যাপচার হবে",
  sitesHelp: "একটি সাইটের সাবডোমেইনও এর অন্তর্ভুক্ত (example.com দিলে dl.example.com-ও)।",
  siteLabel: "সাইট",
  addSite: "সাইট যোগ করুন",
  removeSite: "{site} সরান",
  noSites: "এখনো কোনো সাইট নেই। একটি যোগ না করা পর্যন্ত কিছুই ক্যাপচার হয় না।",
  excludedTitle: "যেখান থেকে কখনো ক্যাপচার হবে না",
  excludedHelp: "উপরের সাইটগুলোর চেয়ে বাদ দেওয়া তালিকা অগ্রাধিকার পায়।",
  addExcluded: "বাদ দিন",
  noExcluded: "কিছু বাদ দেওয়া নেই।",
  invalidSite: "example.com-এর মতো একটি সাইটের নাম লিখুন।",
  sitePermissionDenied: "ব্রাউজার {site}-এ প্রবেশের অনুমতি দেয়নি। এটি যোগ হয়নি।",
  filtersTitle: "ফিল্টার",
  extensionsLabel: "ফাইলের ধরন (কমা দিয়ে আলাদা, খালি = সব)",
  mimeLabel: "MIME টাইপ (কমা দিয়ে আলাদা, খালি = সব)",
  minSizeLabel: "সর্বনিম্ন আকার, MB-তে (0 = যেকোনো)",
  filtersHelp:
    "ফিল্টারের জন্য দরকারি ধরন বা আকার অজানা হলে ডাউনলোডটি ক্যাপচার হয় না। প্রোগ্রাম ও ইনস্টলার সবসময় ব্রাউজারে থাকে।",
  saveFilters: "ফিল্টার সংরক্ষণ করুন",
  saved: "সংরক্ষিত হয়েছে।",
  activityTitle: "সাম্প্রতিক কার্যক্রম",
  noActivity: "এখনো কিছু ক্যাপচার হয়নি।",
  clearActivity: "কার্যক্রম মুছুন",
  job: "Downova জব {job}",
  popupOn: "{count}টি সাইটের জন্য স্বয়ংক্রিয় ক্যাপচার চালু।",
  popupOff: "স্বয়ংক্রিয় ক্যাপচার বন্ধ।",
  openSettings: "স্বয়ংক্রিয় ক্যাপচারের সেটিংস",
  "state.evaluating": "যাচাই চলছে",
  "state.browser_owned": "ব্রাউজারে রাখা হয়েছে",
  "state.sending": "Downova-তে পাঠানো হচ্ছে",
  "state.accepted": "Downova গ্রহণ করেছে",
  "state.uncertain": "নিশ্চিত হয়নি",
  "state.failed": "ব্রাউজারে রাখা হয়েছে",
  "browser.running": "",
  "browser.cancelled_by_downova": "ব্রাউজারের ডাউনলোড বাতিল হয়েছে; Downova এটি চালিয়ে যাচ্ছে।",
  "browser.finished_in_browser":
    "ব্রাউজার আগে শেষ করেছে, তাই দুটি কপি আছে: ব্রাউজারের ও Downova-র।",
  "browser.cancelled_by_user": "আপনি ব্রাউজারের ডাউনলোড বাতিল করেছেন; Downova-র জব এখনো আছে।",
  "browser.kept": "ব্রাউজারের ডাউনলোড যেমন ছিল তেমনই রাখা হয়েছে।",
  uncertain:
    "Downova নিশ্চিত করেনি। ব্রাউজার ডাউনলোড চালিয়ে যাচ্ছে। Downova-ও এটি যোগ করে থাকতে পারে (সর্বোচ্চ একবার), তাই দুটি কপি হতে পারে।",
  "reason.unsupported_scheme":
    "এই ধরনের লিংক (blob:, data: ইত্যাদি) শুধু ব্রাউজারই সংরক্ষণ করতে পারে।",
  "reason.invalid_link": "লিংকটি সাধারণ পাবলিক http(s) লিংক নয়।",
  "reason.incognito": "প্রাইভেট উইন্ডো কখনো ক্যাপচার হয় না।",
  "reason.site_not_allowed": "এই সাইটটি আপনার তালিকায় নেই।",
  "reason.site_excluded": "এই সাইটটি বাদ দেওয়া আছে।",
  "reason.permission_missing": "ব্রাউজার এই সাইটে প্রবেশের অনুমতি দেয়নি। সাইটটি আবার যোগ করুন।",
  "reason.browser_warning":
    "ব্রাউজার এই ডাউনলোড নিয়ে সতর্ক করেছে, তাই ব্রাউজারের নিজস্ব যাচাইই প্রযোজ্য।",
  "reason.executable": "প্রোগ্রাম ও ইনস্টলার সবসময় ব্রাউজারে থাকে।",
  "reason.no_request_evidence":
    "এই ডাউনলোডের পেছনের অনুরোধ Downova দেখতে পায়নি, তাই এটি সাধারণ পাবলিক ডাউনলোড কিনা জানা যায়নি।",
  "reason.ambiguous_request": "একাধিক অনুরোধ এই ডাউনলোডের সাথে মিলেছে; কোনটি তা বোঝা যায়নি।",
  "reason.not_get": "ডাউনলোডটি ফর্ম বা অন্য কোনো non-GET অনুরোধ থেকে এসেছে।",
  "reason.sent_cookies": "ব্রাউজার এই ডাউনলোডের জন্য কুকি পাঠিয়েছে (সাইন-ইন লাগতে পারে)।",
  "reason.sent_credentials": "ব্রাউজার এই ডাউনলোডের জন্য সাইন-ইনের তথ্য পাঠিয়েছে।",
  "reason.unexpected_status": "সার্ভারের উত্তর সাধারণ সম্পূর্ণ ফাইল ছিল না।",
  "reason.redirect_not_allowed": "ডাউনলোডটি আপনার তালিকার বাইরের সাইটে রিডাইরেক্ট হয়েছে।",
  "reason.mime_unknown": "ফাইলের ধরন অজানা, অথচ MIME ফিল্টার দেওয়া আছে।",
  "reason.mime_filtered": "ফাইলের ধরন আপনার MIME ফিল্টারের সাথে মেলে না।",
  "reason.type_filtered": "ফাইলের ধরন আপনার ফাইল-ধরন ফিল্টারের সাথে মেলে না।",
  "reason.size_unknown": "আকার অজানা, অথচ সর্বনিম্ন আকার দেওয়া আছে।",
  "reason.too_small": "ফাইলটি আপনার সর্বনিম্ন আকারের চেয়ে ছোট।",
  "reason.busy": "Downova তখন তিনটি ডাউনলোড নিচ্ছিল।",
  "reason.capture_disabled": "স্বয়ংক্রিয় ক্যাপচার বন্ধ করা হয়েছিল।",
  "reason.permission_revoked": "একটি অনুমতি সরিয়ে নেওয়া হয়েছে।",
  "reason.downova_unavailable": "Downova পাওয়া যাচ্ছে না; ডাউনলোড ব্রাউজারেই আছে।",
  "reason.downova_rejected": "Downova এই লিংক গ্রহণ করেনি; ডাউনলোড ব্রাউজারেই আছে।",
  menuTitle: "Downova দিয়ে ডাউনলোড করুন",
  "popup.title": "Downova",
  "popup.hint":
    "লিংকে ডান-ক্লিক করে “Downova দিয়ে ডাউনলোড করুন” বাছুন। শুধু পাবলিক http ও https লিংক পাঠানো হয়; এতে ব্রাউজারের নিজের ডাউনলোড কখনো বদলায় না।",
  "popup.serviceHeading": "সার্ভিস",
  "popup.check": "আবার যাচাই করুন",
  "popup.captureHeading": "স্বয়ংক্রিয় ক্যাপচার",
  "popup.recentHeading": "সাম্প্রতিক হস্তান্তর",
  "popup.loading": "লোড হচ্ছে…",
  "popup.empty": "এখনো কিছু পাঠানো হয়নি।",
  "popup.clear": "তালিকা মুছুন",
  "service.checking": "Downova চালু আছে কিনা যাচাই চলছে…",
  "service.ready": "সার্ভিস প্রস্তুত। Downova {version} চালু আছে (ব্যাকএন্ড {backend})।",
  "service.host_missing":
    "সার্ভিস পাওয়া যাচ্ছে না। এই ব্রাউজারের জন্য Downova-র ব্রাউজার সহায়ক ইনস্টল নেই। Downova ইনস্টল বা মেরামত করে আবার যাচাই করুন।",
  "service.unavailable": "সার্ভিস পাওয়া যাচ্ছে না। Downova অ্যাপ খুলে আবার যাচাই করুন।",
  "service.no_answer":
    "সার্ভিসের অবস্থা অজানা। Downova-র ব্রাউজার সহায়ক উত্তর দেয়নি। আবার চেষ্টা করুন।",
  "service.unexpected":
    "সার্ভিসের অবস্থা অজানা। Downova-র ব্রাউজার সহায়ক অপ্রত্যাশিত উত্তর দিয়েছে।",
  "handoff.state.sending": "পাঠানো হচ্ছে",
  "handoff.state.accepted": "Downova গ্রহণ করেছে",
  "handoff.state.rejected": "গ্রহণ করা হয়নি",
  "handoff.state.unavailable": "Downova পাওয়া যাচ্ছে না",
  "handoff.state.uncertain": "নিশ্চিত হয়নি",
  "handoff.code.sending": "Downova-তে পাঠানো হচ্ছে…",
  "handoff.code.accepted":
    "Downova লিংকটি গ্রহণ করেছে। এর মানে এখনো স্থানান্তর শুরু বা শেষ হওয়া নয়; Downova অ্যাপে দেখুন।",
  "handoff.code.rejected": "Downova এই লিংক গ্রহণ করেনি।",
  "handoff.code.rejected_invalid": "এটি সঠিক লিংক নয়।",
  "handoff.code.rejected_scheme": "শুধু http ও https লিংক Downova-তে পাঠানো যায়।",
  "handoff.code.rejected_credentials": "ব্যবহারকারীর নাম বা পাসওয়ার্ডসহ লিংক ব্রাউজারেই থাকে।",
  "handoff.code.rejected_too_long": "লিংকটি খুব লম্বা।",
  "handoff.code.rejected_no_host": "লিংকে সার্ভারের নাম নেই।",
  "handoff.code.rejected_spaces": "লিংকে ফাঁকা জায়গা বা নিয়ন্ত্রণ অক্ষর আছে।",
  "handoff.code.unavailable":
    "Downova পাওয়া যাচ্ছে না। কিছুই যোগ হয়নি; Downova চালু হলে আবার চেষ্টা করুন।",
  "handoff.code.host_missing":
    "এই ব্রাউজারের জন্য Downova-র ব্রাউজার সহায়ক ইনস্টল নেই। Downova ইনস্টল বা মেরামত করে আবার চেষ্টা করুন।",
  "handoff.code.uncertain":
    "কোনো নিশ্চিতকরণ আসেনি। ডাউনলোডটি যোগ হয়ে থাকতেও পারে; জানতে আবার চেষ্টা করুন (দ্বিতীয় কপি তৈরি হয় না)।",
  "handoff.retry": "আবার চেষ্টা",
  "handoff.retryLabel": "{link} আবার পাঠান",
  "handoff.dismiss": "সরান",
  "handoff.dismissLabel": "{link} সরান",
};

export type Locale = "en" | "bn";

export function localeOf(uiLanguage: string): Locale {
  return uiLanguage.toLowerCase().startsWith("bn") ? "bn" : "en";
}

export function translator(locale: Locale) {
  const table = locale === "bn" ? bn : en;
  return (key: Key, values: Record<string, string | number> = {}): string =>
    table[key].replace(/\{(\w+)\}/g, (_, name: string) => String(values[name] ?? `{${name}}`));
}

export type Translate = ReturnType<typeof translator>;

export const stateKey = (s: CaptureState): Key => `state.${s}`;
export const browserKey = (b: BrowserSide): Key => `browser.${b}`;
export const reasonKey = (r: Reason): Key => `reason.${r}`;
export const handoffStateKey = (s: HandoffState): Key => `handoff.state.${s}`;
export const handoffCodeKey = (c: HandoffCode): Key => `handoff.code.${c}`;
export const serviceKey = (c: ServiceCode | "checking"): Key => `service.${c}`;
export type { Key as MessageKey };

/** Every key exists in both languages (also enforced by the `Record<Key, string>` type). */
export const KEYS = Object.keys(en) as Key[];
export const CATALOGS = { en, bn };
