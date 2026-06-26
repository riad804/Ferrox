import SwiftUI
import PhotosUI
// The UniFFI-generated `Generated/Ferrox.swift` is compiled into this app target,
// so its types (ImageSession, RawImage, …) are in-module — no `import Ferrox`.
// That file imports the C module `FerroxFFI` from the xcframework internally.

/// Minimal M1 photo editor proving the ferrox SDK end-to-end on iOS:
///   pick → ImageSession(Data) → chained edits in Rust → render → save JPEG.
///
/// The original picked bytes are kept; every edit re-applies the full chain from
/// a clean session (simple + correct for M1).
struct ContentView: View {
    @State private var originalData: Data?
    @State private var edits: [(ImageSession) throws -> Void] = []
    @State private var preview: UIImage?
    @State private var pickerItem: PhotosPickerItem?
    @State private var status: String = ""

    var body: some View {
        VStack(spacing: 12) {
            HStack {
                PhotosPicker("Pick photo", selection: $pickerItem, matching: .images)
                    .buttonStyle(.borderedProminent)
                Button("Save") { save() }
                    .buttonStyle(.bordered)
                    .disabled(originalData == nil)
            }

            ZStack {
                Color(.secondarySystemBackground)
                if let preview {
                    Image(uiImage: preview)
                        .resizable()
                        .scaledToFit()
                } else {
                    Text("No image").foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .clipShape(RoundedRectangle(cornerRadius: 12))

            toolbar
            Text(status).font(.caption).foregroundStyle(.secondary)
        }
        .padding()
        .onChange(of: pickerItem) { item in load(item) }
    }

    private var toolbar: some View {
        VStack(spacing: 8) {
            HStack {
                editButton("Brightness +") { try $0.brightness(delta: 20) }
                editButton("Contrast +") { try $0.contrast(factor: 1.2) }
            }
            HStack {
                editButton("Grayscale") { try $0.grayscale() }
                editButton("Blur") { try $0.blur(sigma: 2.0) }
            }
            HStack {
                editButton("Rotate 90°") { try $0.rotate(degreesCw: 90) }
                editButton("Crop square") { try $0.cropCenterSquare() }
            }
            Button("Reset") { edits.removeAll(); render() }
                .frame(maxWidth: .infinity)
                .buttonStyle(.bordered)
                .disabled(originalData == nil)
        }
    }

    private func editButton(
        _ title: String, _ edit: @escaping (ImageSession) throws -> Void
    ) -> some View {
        Button(title) {
            guard originalData != nil else { return }
            edits.append(edit)
            render()
        }
        .frame(maxWidth: .infinity)
        .buttonStyle(.bordered)
        .disabled(originalData == nil)
    }

    // MARK: - SDK round-trip

    private func load(_ item: PhotosPickerItem?) {
        guard let item else { return }
        Task {
            if let data = try? await item.loadTransferable(type: Data.self) {
                originalData = data
                edits.removeAll()
                render()
            } else {
                status = "Could not load image"
            }
        }
    }

    /// Rebuild the edited image off the main thread and show it.
    private func render() {
        guard let src = originalData else { preview = nil; return }
        Task.detached(priority: .userInitiated) {
            do {
                let session = try ImageSession(imageData: src)
                for edit in await edits { try edit(session) }
                let raw = try session.toRgba8()
                let image = Self.uiImage(from: raw)
                await MainActor.run { self.preview = image }
            } catch {
                await MainActor.run { self.status = "Render error: \(error)" }
            }
        }
    }

    private func save() {
        guard let src = originalData else { return }
        Task.detached(priority: .userInitiated) {
            do {
                let session = try ImageSession(imageData: src)
                for edit in await edits { try edit(session) }
                let jpeg = try session.exportJpeg(quality: 90)
                if let img = UIImage(data: jpeg) {
                    UIImageWriteToSavedPhotosAlbum(img, nil, nil, nil)
                    await MainActor.run { self.status = "Saved to Photos" }
                }
            } catch {
                await MainActor.run { self.status = "Save error: \(error)" }
            }
        }
    }

    /// Build a UIImage from tightly-packed RGBA8 produced by ferrox.
    private static func uiImage(from raw: RawImage) -> UIImage? {
        let w = Int(raw.width), h = Int(raw.height)
        guard w > 0, h > 0, raw.pixels.count == w * h * 4 else { return nil }
        // CFData retains the bytes, so the provider stays valid for the image's life.
        guard let provider = CGDataProvider(data: raw.pixels as CFData) else { return nil }
        let cs = CGColorSpaceCreateDeviceRGB()
        let info = CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue)
        guard let cg = CGImage(
            width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32,
            bytesPerRow: w * 4, space: cs, bitmapInfo: info,
            provider: provider, decode: nil, shouldInterpolate: false,
            intent: .defaultIntent
        ) else { return nil }
        return UIImage(cgImage: cg)
    }
}
