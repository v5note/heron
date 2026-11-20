# ui/src/assets

The rail logo in the top-left corner of the app shell.

| File | Origin | Used by |
| --- | --- | --- |
| `logo-light.png` | **derived** from `docs/icons/icon-master-2.png` | light theme |
| `logo-dark.png` | **derived** from `docs/icons/icon-master-6.png` | dark theme |

`App.vue` shows exactly one of them, chosen by `<html data-theme>`, so the right logo is on
screen at first paint — no JS, no flash of the other one.

## Why both are derived

The masters are 1024-class artwork, not 34 px tiles:

- `icon-master-2.png` is a white heron. As PNG it is white ink on transparency, which vanishes
  on the light rail. It is rendered onto the neutral dark plate instead, keeping the white bird
  as drawn.
- `icon-master-6.png` is near-black ink on a fully transparent plate, which vanishes on the dark
  rail. Its ink is recoloured to warm parchment and the plate filled with the same neutral dark.

Both are recentred on the bird's bounding box: the raw canvases carry asymmetric padding (the
light one runs off the right edge), so the bird would sit visibly off-centre in a square tile.

```bash
python3 scripts/make-rail-logo.py
```

The masters in `docs/icons/` are never modified. To swap artwork, replace the master and re-run.

## Bundling

`vite.config.ts` keeps both out of the JS bundle (`assetsInlineLimit` returns `false` for
`logo*.png`): they are emitted as files rather than inlined as base64 in the entry chunk.
