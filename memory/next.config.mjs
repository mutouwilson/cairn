/** @type {import('next').NextConfig} */
const isProd = process.env.NODE_ENV === "production";
const internalHost = process.env.TAURI_DEV_HOST || "localhost";

const nextConfig = {
  output: "export",
  images: { unoptimized: true },
  reactStrictMode: true,
  trailingSlash: true,
  // In dev Tauri loads from the Next.js dev server. In prod Tauri serves the
  // static export bundle from `out/` directly.
  ...(isProd ? {} : { assetPrefix: `http://${internalHost}:3000` }),
};

export default nextConfig;
