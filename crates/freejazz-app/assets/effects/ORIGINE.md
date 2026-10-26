# Origine et licences des icônes d'effets

## Index
- Icônes SVG de deux bibliothèques libres : Carbon (Apache-2.0) et Ionicons (MIT).
- Pairage mode d'effet / icône : `src/sections/lighting/icons.rs` (mêmes associations que le configurateur d'Ajazz).
- Les données de `data/` (effets, touches) sont des faits relevés dans le configurateur d'Ajazz.

## Bibliothèques
Le configurateur web d'Ajazz (https://ajazz.driveall.cn/, page « Éclairage ») représente chaque effet
intégré par une icône de bibliothèque libre, obtenue via Iconify. Freejazz reprend les mêmes
icônes, récupérées depuis ces bibliothèques (aucun code du site n'est repris).

| Préfixe des fichiers | Bibliothèque | Licence | Auteur |
| --- | --- | --- | --- |
| `carbon_*.svg` | [Carbon](https://github.com/carbon-design-system/carbon) | Apache-2.0 (<https://www.apache.org/licenses/LICENSE-2.0>) | IBM |
| `ion_*.svg` | [Ionicons](https://github.com/ionic-team/ionicons) | MIT (<https://github.com/ionic-team/ionicons/blob/main/LICENSE>) | Ben Sperry / Ionic |

Les icônes Hugeicons et Tabler, qui n'étaient pas utilisées par Freejazz, ont été retirées du dépôt
pour ne garder que des licences clairement redistribuables.

## Données des touches et des effets
Les tables de `data/ak980max/` (noms et codes d'effets, codes de touches, fonctions, disposition)
sont des faits techniques relevés dans le configurateur d'Ajazz pour que Freejazz parle au clavier ;
Freejazz n'est ni affilié à Ajazz ni approuvé par cette marque.

## Police
Inter (SIL Open Font License 1.1) : voir `../fonts/Inter-OFL.txt`.
