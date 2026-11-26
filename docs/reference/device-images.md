# Device images

The camera and mount cards (Camera and Aim tabs) show a photo of the connected device when the catalog (`crates/server/data/catalog.json`) has one, and a drawn placeholder (`web/public/devices/camera.svg`, `mount.svg`) otherwise.

- Photos are in `web/public/devices/`, as cut-outs (transparent background, WebP, about 360 px wide). Each catalog entry names its file in `image` and the page it came from in `image_source`.
- They are the manufacturers' product photos, taken from their own web pages (ZWO, Player One Astronomy, Sky-Watcher; the iOptron HAE69C from a retailer's product page) and shown here only to identify the device the user is connected to. **They remain the property of ZWO, Player One Astronomy, Sky-Watcher and iOptron** and are not covered by this project's licence. If a rights holder asks, they are removed.
- To add one: download the product photo, remove a white background if it has one (flood from the corners, not a colour key, so white labels stay), save it as WebP, and add `image` and `image_source` to the catalog entry. A device without one keeps the placeholder.
