# Downova B2 — দ্রুত শুরু (পরীক্ষকের জন্য)

এটা শুধু মূল যাত্রাপথ: ইনস্টল, ছয়টা download একসাথে, আর hash যাচাই। পুরো পরীক্ষার তালিকা
`CHECKLIST.md`-এ।

## Kit কোথা থেকে পাবেন
1. GitHub-এ **Actions → "Acceptance kit (normal packages, internal)"** খুলুন। একজন maintainer "Run workflow" চালালে
   তবেই তৈরি হয়। সবুজ (সফল) run-টা খুলুন।
2. Run-এর **Summary**-তে নিজের platform-এর সারি দেখুন। "Kit ZIP validation"-কে `success` দেখাতে হবে।
3. নিচে **Artifacts** থেকে নিজের platform-এরটা নামান। নামে platform, architecture আর source commit থাকে:
   - `downova-acceptance-kit-windows-x86_64-<commit>`
   - `downova-acceptance-kit-macos-aarch64-<commit>`
   - `downova-acceptance-kit-linux-x86_64-<commit>`

   `…-logs` নামেরটা শুধু build log, পরীক্ষার জন্য নয়।
4. Artifact-টা নিজেও একটা zip। খুললে ভেতরে পাবেন `acceptance-kit-b2-<platform>.zip`, তার `.sha256`, আর
   `.json`। এই kit zip-এর hash মেলান:
   - Windows: `Get-FileHash .\acceptance-kit-b2-windows-x86_64.zip -Algorithm SHA256`
   - macOS: `shasum -a 256 acceptance-kit-b2-macos-aarch64.zip`
   - Linux: `sha256sum acceptance-kit-b2-linux-x86_64.zip`

   ফল `.sha256` ফাইলের মানের সাথে মিলতে হবে।
5. Kit zip খুলুন:
   - Windows: right-click → Extract All;
   - macOS: double-click;
   - Linux: `unzip`।

Artifact নামানো বা hash মেলানো মানেই পরীক্ষা পাস নয়। Installed-app আর মানুষের পরীক্ষা হয় শুধু নিচের ধাপ আর
`CHECKLIST.md` চালিয়ে, আর ফল লেখা হয় `RESULTS.csv`-এ।

## আগে যা লাগবে
- **একটা পরিষ্কার VM বা আলাদা test OS account।** নিজের প্রতিদিনের account ব্যবহার করবেন না।
- **Node.js 20 বা নতুন** (fixture চালাতে লাগে), আর Chrome ও Edge।
- **Page zoom আর OS display scaling আলাদা পরীক্ষা:**
  - **Page zoom:** Downova-র ভেতরে Ctrl/⌘ + চাপা।
  - **OS display scaling:** Windows Settings → Display → Scale, macOS Displays, বা GNOME Settings → Displays।

  `RESULTS.csv`-এর notes-এ লিখুন কোনটা ব্যবহার করেছেন।
- **এই kit ফোল্ডার।** প্রথমে একবার যাচাই করুন:

  ```text
  node verify-kit.mjs
  ```

  সব ফাইল `OK` দেখাতে হবে, আর `smoke_feature: false` থাকতে হবে।

## ধাপ (Windows)
1. **Package-এর hash মেলান:**

   ```text
   Get-FileHash .\package\Downova_0.1.0_x64-setup.exe -Algorithm SHA256
   ```

   ফলাফল `KIT-INFO.json`-এর `setup_sha256`-এর সাথে হুবহু মিলতে হবে।
2. **ইনস্টল করুন:** `package\Downova_0.1.0_x64-setup.exe` চালান। এটা per-user ইনস্টল, admin লাগে না। Folder
   পাতায় যে ফোল্ডার দেখায় সেটা লিখে রাখুন (সাধারণত `%LOCALAPPDATA%\Downova`)। সেখানেই `dm.exe` থাকে।
3. **Fixture চালু করুন** (আলাদা একটা PowerShell-এ, kit ফোল্ডার থেকে):

   ```text
   node fixtures\perf\fixture-process.mjs --count 6 --size-mib 256 --port 8766
   ```

   প্রথম লাইনটা (JSON) `evidence\fixture-hashes.json` নামে save করুন। এতে প্রতিটা ফাইলের SHA-256 আছে।
4. **Downova খুলুন।** Window-এ "service connected" দেখাবে।
5. **Extension লোড করুন:** Chrome/Edge → Extensions → Developer mode → Load unpacked → kit-এর `extension`
   ফোল্ডার। Popup-এ "Service ready" দেখাবে।
6. **দুটো limit ৬ করুন:**
   - Options → Simultaneous downloads → 6 → Save;
   - তারপর **Change queue limit** → Main = 6 → Set limit।

   "Main can run at once 6" দেখাবে।
7. **ছয়টা ফাইল যোগ করুন:** Downloads → Add URL দিয়ে একে একে
   `http://127.0.0.1:8766/f0.bin?slow=100` থেকে `…/f5.bin?slow=100`।
8. **দেখুন:** প্রথম ফাইল শেষ হওয়ার আগেই ছয়টারই শতাংশ বাড়ছে কি না। একটা screenshot নিন।
9. **শেষ হলে hash মেলান:**

   ```text
   Get-FileHash "$env:USERPROFILE\Downloads\f0.bin" -Algorithm SHA256
   ```

   প্রতিটা ফাইলের জন্য এভাবে করুন, আর `fixture-hashes.json`-এর মানের সাথে মেলান।
10. **ফল লিখুন:** `RESULTS.csv`-এ C1–C3 আর C11-এর সারিতে লিখুন: platform, OS ও browser version, package SHA-256,
    observer (`human`), ফলাফল (`PASSED`/`FAILED`), আর evidence ফাইলের নাম।

## macOS ও Linux
- **Hash:** macOS-এ `shasum -a 256 <file>`, Linux-এ `sha256sum <file>`।
- **ইনস্টল (macOS):** `package/`-এর `.dmg` খুলে Downova-কে **Applications**-এ টেনে নিন, তারপর একবার খুলুন।
  - App স্থায়ীভাবে `/Applications/Downova.app`-এ থাকা চাই; browser helper ওই জায়গা দিয়েই register হয়।
  - Build-টা ad-hoc signed, notarised নয়। প্রথমবার খুলতে right-click → Open লাগতে পারে।
- **ইনস্টল (Linux):** `sudo apt install ./package/<.deb ফাইল>`, তারপর desktop user হিসেবে একবার খুলুন।
  - AppImage চলার সময় browser helper register করে না, তাই browser-এর সব সারি `.deb` দিয়েই করতে হবে।
- **কোন ফাইল আছে:** kit-এ ঠিক কোন package ফাইল আছে, তা এই ফাইলের একদম উপরের তৈরি-করা অংশে আর
  `KIT-INFO.json`-এ লেখা আছে। অন্য platform-এর জন্য সেই platform-এর kit লাগবে (`missing_platforms`)।

## মনে রাখবেন (প্রত্যাশিত আচরণ)
- **Limit কমালে** চলমান download থামে না। নতুনগুলো অপেক্ষা করে।
- **Limit বাড়ালে** নতুন download শুরু হয় শুধু খালি জায়গা থাকলে।
- **Queue stop/start** করলে হাতে pause করা download pause-ই থাকে।
- **"Exit and stop downloads"-এর পর** আবার খুললে অসমাপ্ত download নিজে থেকে চালু হয় না। Resume চাপতে হয়।

## শেষে
- `RESULTS.csv` আর `evidence\` ফোল্ডার রেখে দিন।
- Download করা ফাইলগুলো hash যাচাইয়ের পরেই মুছবেন।
- Test VM-এর snapshot ফেলে দিন, বা test account সরিয়ে দিন।
