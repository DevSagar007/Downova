# Browser store submission (v1.0)

The extension is published to the **Chrome Web Store**, **Microsoft Edge Add-ons** and **addons.mozilla.org (AMO)**.
The owner creates the accounts and uploads. Credentials and API keys never go into the repository or a chat.

## 1. Build the packages

```bash
pnpm store:pack
```

`dist-store/` then holds:

| File | Upload to |
|---|---|
| `downova-chromium-<version>.zip` | Chrome Web Store **and** Edge Add-ons (the same file) |
| `downova-firefox-<version>.zip` | AMO, "On this site" (listed), for Mozilla's signature |
| `SHA256SUMS.txt` | Kept with the release notes |

**What the script checks:** the builds are the normal ones, with no test hooks, no source maps and capture
permissions only optional.
- The Chromium manifest has **no `key`**, because each store assigns its own item ID.
- The Firefox manifest carries the distributed add-on ID `{20566e36-cebf-40ec-960b-160afd8c6ded}`. This ID is
  permanent once published.

## 2. After the first upload: let the app accept the store extensions

The desktop's browser helper only answers extensions it knows (`crates/browser-host/src/lib.rs`).
- **Firefox:** the distributed ID is already allowed.
- **Chrome Web Store and Edge Add-ons:** each gives the item a 32-letter ID at the first upload. Add both to
  `STORE_EXTENSION_IDS`, then release a desktop build with them. Until then, a store-installed extension shows
  "Downova is not available".
- **Optional, for development:** copy the Chrome Web Store item's public key from the developer dashboard (Package →
  public key) into `CHROMIUM_DEV_KEY`. Development builds then have the store ID too.

## 3. Listing text

### Name
Downova

### Short description
- **English (132 characters at most):** Send links to the free Downova download manager on your computer: resume, queues,
  schedules. No account, no ads.
- **Bangla:** আপনার কম্পিউটারের ফ্রি Downova ডাউনলোড ম্যানেজারে লিংক পাঠান: resume, queue, schedule। কোনো অ্যাকাউন্ট নেই,
  বিজ্ঞাপন নেই।

### Detailed description (English)
Downova is a free, account-free download manager for Windows, macOS and Linux. This extension connects your browser to
the Downova app on the same computer.

- **Right-click any link → "Download with Downova":** the app downloads it with pause, resume, verified resume after
  a restart, queues, schedules and speed limits.
- **Automatic capture** (optional, Chrome and Edge): hand plain public downloads from the sites you choose to Downova.
  It is off until you turn it on, and it only covers sites you add.
- **Signed-in downloads** (optional, Chrome and Edge): one explicit menu action passes that one site's sign-in to
  Downova for that download.
- **Private windows are never captured.**
- No account, ads, analytics or tracking. Links go only to the Downova app on your computer.

The Downova desktop app must be installed (it registers the browser connection).

### Detailed description (Bangla)
Downova একটি ফ্রি, অ্যাকাউন্ট ছাড়া ডাউনলোড ম্যানেজার (Windows, macOS, Linux)। এই এক্সটেনশন আপনার ব্রাউজারকে একই কম্পিউটারের
Downova অ্যাপের সাথে যুক্ত করে।

- **যেকোনো লিংকে ডান ক্লিক → "Download with Downova":** pause, resume, রিস্টার্টের পরেও যাচাই করা resume, queue,
  schedule আর speed limit সহ ডাউনলোড।
- **স্বয়ংক্রিয় ক্যাপচার** (ঐচ্ছিক, Chrome ও Edge): আপনার বেছে নেওয়া সাইটের সাধারণ পাবলিক ডাউনলোড Downova-তে যায়। আপনি চালু
  না করা পর্যন্ত বন্ধ থাকে।
- **সাইন-ইন করা ডাউনলোড** (ঐচ্ছিক, Chrome ও Edge): একটি নির্দিষ্ট মেনু অ্যাকশন শুধু সেই সাইটের সাইন-ইন সেই ডাউনলোডের জন্য পাঠায়।
- **প্রাইভেট উইন্ডো কখনো ক্যাপচার হয় না।**
- অ্যাকাউন্ট, বিজ্ঞাপন, analytics বা tracking নেই। লিংক শুধু আপনার কম্পিউটারের Downova অ্যাপে যায়।

Downova ডেস্কটপ অ্যাপ ইনস্টল থাকতে হবে।

### Category
Productivity (Chrome, Edge); Download Management (AMO).

### Privacy policy URL
`https://github.com/DevSagar007/Downova/blob/main/PRIVACY.md`

## 4. Store questionnaire answers

**Single purpose:** hand links the user chooses to the Downova download manager on the same computer.

| Permission | Why it is needed |
|---|---|
| `contextMenus` | The "Download with Downova" item on links |
| `nativeMessaging` | Talk to the Downova app on the same computer (the only destination of a link) |
| `storage` | Remember capture settings and the last 20 hand-off outcomes, shown in the popup and options |
| `downloads` (optional, Chromium) | Automatic capture: see a download the browser started on a chosen site, and cancel it only after Downova accepted it |
| `webRequest` (optional, Chromium; non-blocking) | Automatic capture: confirm a download is a plain public GET (no cookies sent, every redirect inside the chosen sites) |
| `cookies` (optional, Chromium) | Signed-in downloads: one explicit action passes that one site's cookies to Downova for that download |
| Host permissions (optional, per site) | Only for the sites the user adds for automatic capture or signs in to |

**Remote code:** none. Every script is in the package.

**Data use, as disclosed in the stores:**
- **What is handled:** web addresses of the links the user chooses (website content and activity). With the sign-in
  action, that site's authentication cookies.
- **Where it goes:** only to the user's local Downova app. It is never sold, never transferred to third parties and
  never used for advertising or credit decisions.
- **Firefox:** `data_collection_permissions` is `browsingActivity` (required), for the same reason.

## 5. Assets the owner prepares

- **Icon:** 128×128, already in the package (`apps/extension/public/icons/icon-128.png`, the desktop app's icon).
- **Screenshots:** 1280×800 of the context menu, the popup and the options page, in English and Bangla.
- **Chrome promo tile:** 440×280 (optional).
