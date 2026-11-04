# Downova B2 — দ্রুত শুরু (পরীক্ষকের জন্য)

এটা শুধু মূল যাত্রাপথ: ইনস্টল, ছয়টা download একসাথে, আর hash যাচাই। পুরো পরীক্ষার তালিকা
`CHECKLIST.md`-এ।

## আগে যা লাগবে
- **একটা পরিষ্কার VM বা আলাদা test OS account।** নিজের প্রতিদিনের account ব্যবহার করবেন না।
- **Node.js 20 বা নতুন** (fixture চালাতে লাগে), আর Chrome ও Edge।
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
- **ইনস্টল:** macOS-এ `.dmg` → Applications-এ টেনে নিন → একবার খুলুন। Linux-এ
  `sudo apt install ./downova_0.1.0_amd64.deb`, তারপর desktop user হিসেবে একবার খুলুন। AppImage দিয়ে browser
  helper register হয় না।
- **এই kit-এ macOS/Linux package নেই** (`KIT-INFO.json` → `missing_platforms`)। সেগুলো আলাদাভাবে বানাতে বা
  সংগ্রহ করতে হবে, আর তাদের hash একইভাবে রেকর্ড করতে হবে।

## মনে রাখবেন (প্রত্যাশিত আচরণ)
- **Limit কমালে** চলমান download থামে না। নতুনগুলো অপেক্ষা করে।
- **Limit বাড়ালে** নতুন download শুরু হয় শুধু খালি জায়গা থাকলে।
- **Queue stop/start** করলে হাতে pause করা download pause-ই থাকে।
- **"Exit and stop downloads"-এর পর** আবার খুললে অসমাপ্ত download নিজে থেকে চালু হয় না। Resume চাপতে হয়।

## শেষে
- `RESULTS.csv` আর `evidence\` ফোল্ডার রেখে দিন।
- Download করা ফাইলগুলো hash যাচাইয়ের পরেই মুছবেন।
- Test VM-এর snapshot ফেলে দিন, বা test account সরিয়ে দিন।
