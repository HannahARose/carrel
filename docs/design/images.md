# Design: reading images

Status: proposed 2026-10-30.

## What this adds

Today an image's text is whatever OCR finds in it. That works for screenshots of documents, but a photo yields nothing, so the model drafts its card from the file name alone: `IMG_0550.jpeg` is a guess. On macOS 27, Apple's on-device model accepts images in prompts (`Attachment(imageURL:)`, and `SystemLanguageModel.default.capabilities.contains(.vision)`); a test on a stock wallpaper gave a structured title and summary in 2.3 seconds. The Vision framework classifies images into about 1,300 categories without any model, and every photo carries metadata: when it was taken, with what, and often where.

This design reads all three in the Apple provider, so every image gets real text to be filed by, and photos get their real dates.

## One image reader in the Apple provider

- **A new extract offer, `image`,** in `carrel-provider-apple`, for the image types Vision OCR reads (PNG, JPEG, HEIC, HEIF, TIFF, GIF, BMP, WebP) and RAW photos such as DNG. It's listed before `vision`, so it's tried first; `vision` stays for scanned PDFs and as the fallback.
- **It does four things,** and returns them as one text, so drafting, routing and rating need no changes, and a second-opinion model that can't see images (Gemma) still reads what Apple's model saw:
  1. **Metadata** through ImageIO: when it was taken (`DateTimeOriginal`), the camera or app, the size, and whether it has a location.
  2. **Labels** from Vision's `VNClassifyImageRequest`: the most confident few, such as `document`, `screenshot`, `dog`, `beach`.
  3. **Text** from Vision OCR, as now.
  4. **A description** from Apple's model, given the image and the labels: one or two sentences on what it shows.
- **The text looks like:**

  ```text
  Image: photo, 4032 × 3024, taken 2026-06-24 16:28 on an iPhone 15 Pro, with a location
  Shows: A golden retriever lying on a sandy beach at sunset, looking at the camera.
  Labels: dog, beach, sunset, outdoor
  Text in the image: (none)
  ```

- **The method is `described`** when Apple's model wrote a description, `ocr` otherwise, so review can say how an image was read.

## When parts aren't available

- **Without image input** (macOS before 27, or Apple Intelligence off), the description is left out and the rest still helps: metadata and labels take well under a second and need no model.
- **A guardrail refusal** of the description, as Apple's model refuses some health documents today, leaves the description out and records the refusal as an attempt, as other declines are.
- **Very large images are scaled down** to 1,024 pixels on the long side for the description, since an image's tokens come out of the model's 4,096-token context and larger ones are slower. OCR and labels use the full image.
- **The description is its own request,** separate from drafting the card, so the image never competes with the card's prompt for context.

## Dates

- **The date taken becomes the card's `created` date** when the model leaves it empty, and the model is told it in the text either way. Photos and screenshots have exact dates; today's models got dates right 45–77% of the time.
- **The extraction result gets an optional `created` field** (`docs/providers.md`, `text/extract`), so the date reaches the card step as a value, not just as words to read. Providers that don't know a date leave it out.

## Location and privacy

- **Coordinates never go into the text or the card.** The text says only "with a location"; the model doesn't need the place to describe the picture, and a card is easier to share than a photo's metadata.
- **A photo with a location gets a sensitivity reason,** "has the place it was taken", and the floor rises to `personal` (from `public`), never higher: most phone photos have a location, and raising them all to sensitive would bury the ones that are.
- **Everything stays on the device,** as with all of Carrel's reading: Vision, ImageIO and Apple's on-device model, never Private Cloud Compute.

## Building it

1. The `image` offer in `CarrelAppleProviders`: metadata, labels and OCR, with tests on fixture images (a screenshot, a photo with metadata, a PNG without).
2. The description through Foundation Models, behind the `vision` capability check, scaled and with refusals handled.
3. `created` on extraction results, end to end, and the card step using it.
4. The location reason in sensitivity.
5. A benchmark addition: a handful of invented images (a receipt photo, a screenshot, a whiteboard, a pet photo) with known answers, measured with and without the description.
6. The user guide and `docs/providers.md`.

## Decisions to accept

1. **One `image` offer in the Apple provider that returns metadata, labels, OCR and a description as one text,** rather than passing images to the card, routing and rating calls. Recommended: nothing downstream changes, it's one model call per image, and every model, including a second opinion that can't see images, benefits.
2. **The date taken becomes `created`** when the model gives none, carried as a new optional `created` field on extraction results. Recommended.
3. **Locations stay out of text and cards; a located photo gets a sensitivity reason and at least `personal`.** Recommended.
4. **Scale images to 1,024 pixels for the description, and fall back to metadata and labels** when there's no image input or the model refuses. Recommended.
5. **Add images to the benchmark's test set** before calling it done, so the gain is measured, not assumed. Recommended. The portable path (Gemma 4 E4B through llama.cpp's `mtmd`, for Linux and Windows) is a separate design afterwards.
