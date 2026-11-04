# Bundled Pele

Pele 1.13.0, retrieved from https://registry.npmjs.org/pele/-/pele-1.13.0.tgz.
Upstream: https://github.com/obsidianmd/pele. API: https://pele.run/api.

`pele.global.js` is the unmodified upstream bundle. SHA-256:
`0b0a46c57a5a6971670c09f5ec4066705bd78505f90fd11adf6260325efa0910`.
The upstream license (including the Lucide notice) is in LICENSE.

quiet.md calls render() in an isolated QuickJS runtime, without host bindings.
No browser, npm install, network access, or project script is needed at runtime.
CSS variables resolve from the app theme before native SVG rasterization.
