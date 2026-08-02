# Fond de carte mondial embarqué

`world-z0-5.mbtiles` est le fond de carte que Leyline Studio embarque
([ADR 0059](../../docs/adr/0059-bundled-world-basemap.md)) : Natural Earth I
avec relief ombré et eaux, zoom 0 à 5, 1 365 tuiles JPEG qualité 80, 9,0 Mo.

**Source** : [Natural Earth](https://www.naturalearthdata.com)
`NE1_HR_LC_SR_W` (21 600 × 10 800), **domaine public** — aucune licence à
propager, aucune attribution juridiquement exigée. Le pack en déclare une
quand même dans sa `metadata`, par honnêteté sur la provenance.

Les tuiles rendues par le serveur public d'OpenStreetMap ne peuvent pas
remplacer cette source : leur politique d'usage interdit de les redistribuer.
OSM reste la bonne source pour un pack que l'utilisateur *apporte*, pas pour
un pack qu'on *livre*.

## Le régénérer

Nécessite GDAL (`apt install gdal-bin`) et le raster source (309 Mo) :

```bash
curl -LO https://naciscdn.org/naturalearth/10m/raster/NE1_HR_LC_SR_W.zip
unzip NE1_HR_LC_SR_W.zip

# Web Mercator, monde carré, dimensionné pour que z5 soit au 1:1 avec les
# tuiles : 32 tuiles × 256 px = 8192.
ext=20037508.342789244
gdalwarp -t_srs EPSG:3857 -te -$ext -$ext $ext $ext -ts 8192 8192 \
    -r lanczos -co TILED=YES -co COMPRESS=DEFLATE \
    NE1_HR_LC_SR_W.tif world.tif

gdal_translate -of MBTiles -co TILE_FORMAT=JPEG -co QUALITY=80 \
    world.tif world-z0-5.mbtiles
# Les niveaux inférieurs (z4 à z0) sont des aperçus, pas un second rendu.
gdaladdo -r average world-z0-5.mbtiles 2 4 8 16 32
```

Puis la métadonnée, que GDAL ne sait pas écrire entièrement :

```bash
sqlite3 world-z0-5.mbtiles <<'SQL'
DELETE FROM metadata WHERE name IN ('name','description','type','attribution');
INSERT INTO metadata (name, value) VALUES
  ('name', 'Leyline world basemap'),
  ('description', 'Natural Earth I with shaded relief and water, zoom 0-5'),
  ('type', 'baselayer'),
  ('attribution', 'Natural Earth (public domain)');
VACUUM;
SQL
```

## Ce qui a été mesuré avant de choisir

| Profondeur | Tuiles | JPEG q80 | PNG 32 bits | PNG 8 bits |
|---|---|---|---|---|
| z0–5 | 1 365 | **9,0 Mo** | 26,5 Mo | 73,0 Mo |
| z0–6 | 5 461 | 29,8 Mo | 90,7 Mo | 253,9 Mo |

Le PNG 8 bits, format « léger » attendu, est huit fois plus lourd que le
JPEG sur ce contenu : la quantification s'accommode mal d'un dégradé de
relief. GDAL 3.4 refuse `TILE_FORMAT=WEBP`, cette piste n'a pas de mesure.
