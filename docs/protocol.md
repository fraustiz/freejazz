# Protocole HID de l'Ajazz AK980 Max

Notre description du protocole que parle le configurateur web d'Ajazz (https://ajazz.driveall.cn/) avec l'AK980 Max, établie en lisant le code du site. Rien ici n'est copié du site : ce sont nos propres tableaux et explications. Les données factuelles extraites (disposition, tables de touches, effets) sont dans `data/ak980max/` (voir §8).

## En bref

- Rapports HID de `reportCount` octets (numéro de rapport 0) sur une interface « constructeur » : **64 octets sur l'AK980 Max testé** (interface `MI_02`, page d'usage `0xFF68`), 32 quand le descripteur n'en donne pas ; requête `AA cmd len addr_lo addr_hi x x x` + `reportCount − 8` octets de données (56 ici), réponse `55 cmd …` + autant de données.
- Les blocs (48 à 1024 octets) sont transférés en morceaux de `reportCount − 8` octets (56 sur cet exemplaire, 24 avec des rapports de 32), chacun attendant sa réponse (500 ms, 3 relances).
- Chaque bloc a sa disposition d'octets ci-dessous (§6), avec ce que le site lit et écrit pour **ce modèle** (§5) et ce qui reste **non confirmé — à vérifier sur le clavier**.

## Sources et niveau de confiance

- Code analysé : copie du site téléchargée le 2026-10-20 (hors dépôt), en particulier `layout-classic-0DHAQysC.js` (module protocole, configurations des claviers, pages en composables), `lighting-DzPku7KQ.js` (liste des effets), `lightingPane-*.js`, `musicV2-*.js`, `mt-*.js`, `socd-*.js`, `rs-*.js`, `tgl-*.js`, `dks-*.js`, `macro-*.js`, `settings-*.js`, `performance-*.js`, `calibrationDrawer-*.js`, `keyboard-*.js`, et le pack de langue `fr-*.js` / `en-*.js` pour le sens des libellés.
- Analyse REA : preuve `ev_b41f45ff04a9cd11064e2fff45043d98644b9a1a95551407d69777e576d9b024` (REA 6.7.0, `analyze-javascript-application`). Les détails ci-dessous ont été relus directement dans le code du site (2026-10-24).
- Convention : tout ce qui n'est pas marqué est **confirmé par le code du site** (c'est ce que le site envoie ou décode). **Non confirmé — à vérifier sur le clavier** signale une déduction ou un point que le code ne tranche pas.
- Premier contact avec le clavier, en lecture seule, le 2026-10-24 (tâche 4) : valeurs relevées au §9. Ce qui y a été vérifié est marqué **confirmé sur le clavier**.
- Premières écritures les 2026-10-24 et 25 (tâche 5, `selftest-writes`) : §9.7.

## 1. Matériel et interface HID

- AK980 Max en USB : `VID 0x0C45` (3141), `PID 0x8043` (32835). Le site connaît aussi les PID 32848–32852 sous le nom « AK980 MAX » : leur configuration est identique à celle du 32835 (mêmes touches, mêmes index, mêmes réglages), seul le nom du premier type de switch change (`axis_13`, `axis_31` ou `axis_26` au lieu de `axis_9`).
- Le site choisit la configuration par la clé `vid-pid-nomDuProduit` (`3141-32835-AK980 MAX`), le nom étant la chaîne produit HID.
- Interface utilisée : le site ouvre les périphériques HID dont une collection a une page d'usage parmi `0xFF68`, `0xFF80`, `0xFF60`, `0xFF00`, `0xFF01`, `0xFF1B`. Sur l'AK980 Max c'est **`MI_02`** (page d'usage `0xFF68`, usage `0x61`), qui répond à `GET_DEVICE_INFO` — **confirmé sur le clavier** (§9). Freejazz n'ouvre que ces pages d'usage.
- Taille des rapports : `reportCount` du premier rapport de sortie de la première collection de l'interface, 32 par défaut. Toutes les tailles de morceau en dépendent (`reportCount − 8`). Le site envoie avec le numéro de rapport 0. Sur `MI_02` : **64 octets en entrée et en sortie**, donc des morceaux de 56 octets — **confirmé sur le clavier**. Freejazz lit ce `reportCount` dans le descripteur que Windows reconstitue (`HidD_GetPreparsedData`).
- WebHID fournit les rapports d'entrée sans numéro de rapport. Avec hidapi sous Windows (`windows-native`) : l'écriture commence par l'octet `0x00` (numéro de rapport, 65 octets au total), et la lecture renvoie le rapport sans cet octet (64 octets commençant par `0x55`) — **confirmé sur le clavier**.
- Les interfaces `0xFF67` à grands rapports (animations, écran) ne servent qu'à d'autres modèles. Sur l'AK980 Max, c'est `MI_03` (sortie 4096 octets, rapport de fonction 64 octets) : ni le site ni Freejazz ne l'ouvrent.

## 2. Format des paquets

### 2.1 Requête (hôte → clavier), `reportCount` octets

| Octet | Contenu |
|---|---|
| 0 | `0xAA` |
| 1 | code de commande |
| 2 | longueur utile du morceau (nombre d'octets de données de ce morceau) — ou paramètre (type de remise à zéro, §6.10) |
| 3–4 | adresse du morceau, u16 petit-boutiste : `addrStart + t × taille_morceau` |
| 5–7 | en-tête complémentaire facultatif (au plus 3 octets, copiés à partir de l'octet 5) |
| 6 | `1` sur le dernier morceau d'un transfert, `0` sinon — **sauf** quand l'en-tête complémentaire fait 2 octets ou plus : l'octet 6 vaut alors le 2ᵉ octet complémentaire. Certains transferts désactivent le drapeau (table des macros, §6.6). |
| 8… | données du morceau, puis des zéros jusqu'à `reportCount` |

Variante « en-tête sur mesure » : un en-tête de N octets (3, 4 ou 8) est copié tel quel à partir de l'octet 0 et les données suivent directement à l'octet N ; tout le reste est à zéro. Utilisée par `SET_PROFILE` (`AA 90 n`), la lecture d'une seule touche (`AA 12 04 addr_lo addr_hi 00 00 00`), le calibrage v2 (`AA 69 …`, `AA 6A …`) et les commandes OTA.

Commandes « courtes » sans transfert (`SET_FACTORY_RESET`, `SET_CALIBRATION_ON/OFF`, `SET_SIMULATION_TEST_ON/OFF`) : un seul rapport `AA cmd p 00 00 00 00 00 …` (p en octet 2, octet 6 = 0).

### 2.2 Réponse (clavier → hôte)

| Octet | Contenu |
|---|---|
| 0 | `0x55` (sinon le site ignore le rapport) |
| 1 | code de commande en écho |
| 2 | longueur ou type (lu seulement par `SET_PROFILE` et l'OTA) |
| 3–4 | adresse en écho, u16 petit-boutiste |
| 5–7 | non utilisés par le site |
| 8… | données (24 octets pour des rapports de 32) |

Le site associe une réponse à une requête par le seul code de commande (octet 1), et en plus par l'adresse quand le transfert le demande (`checkAddr`, utilisé par `GET_MAGNETIC_AXIS_STATUS`). Tout autre rapport reçu pendant l'attente est ignoré : c'est ce qui protège des rapports spontanés (§6.14) et des réponses à d'autres commandes.

## 3. Transferts découpés, délais et relances

- Taille de morceau `c = reportCount − 8` : 24 avec des rapports de 32, **56 sur l'unité testée** (ex. 512 octets → 10 morceaux, 1024 → 19). Un bloc de `total` octets fait `n = ⌈total / c⌉` morceaux. Morceau `t` : adresse `addrStart + t·c`, longueur `min(c, total − t·c)`, données `data[t·c … t·c + longueur]` pour une écriture (rien pour une lecture), drapeau de fin si `t = n − 1`.
- Après chaque envoi, le site attend une réponse portant la même commande (et la même adresse si `checkAddr`). Sans réponse dans le délai, il renvoie le même morceau, jusqu'à 3 fois (4 envois au total), puis abandonne.
- Données lues : octets `8…` de chaque réponse, concaténés, tronqués à `total`.
- Abandon : le site ferme et oublie le clavier, affiche une erreur de liaison et revient à l'accueil (sauf pour `0x1C`, dont l'échec est silencieux). Avec 0 relance, il renvoie simplement un résultat vide.
- Mode « sans réponse » (`skipResponse`) : envoi des morceaux sans attente (non utilisé pour ce modèle).

Délais par morceau (`frameVersion` vient de `GET_DEVICE_INFO`) :

| Commande | Délai | Relances |
|---|---|---|
| par défaut (toutes lectures et écritures de blocs) | 500 ms, 2000 ms si `frameVersion == 1` | 3 |
| `GET_DEVICE_INFO` 0x10 | 500 ms (le `frameVersion` n'est pas encore connu) | 3 |
| `SET_KEY` 0x22 | 1000 ms, 2000 ms si `frameVersion == 1` | 3 |
| `SET_CUSTOM_LED_DATA` 0x24 | 2000 ms | 3 |
| `SET_PROFILE` 0x90 | 2000 ms | 3 |
| `GET_MAGNETIC_AXIS_STATUS` 0x68 | 2000 ms | 3 |
| `SET_CALIBRATION_ON_V2/OFF_V2` 0x69/0x6A | 2000 ms | 3 |
| `GET_LED_DATA` 0x32 (flux musique) | 50 ms, 2000 ms si `frameVersion == 1` | 3 |
| `GET_DEFAULT_FN_KEY_MATRIX` 0x1C | 500 ms / 2000 ms | 1 |
| `SET_CALIBRATION_ON` 0x64, `SET_SIMULATION_TEST_ON` 0x66 | une réponse attendue en 1000 ms, pas de relance | 0 |
| `SET_CALIBRATION_OFF` 0x65, `SET_SIMULATION_TEST_OFF` 0x67, `SET_FACTORY_RESET` 0x0F | aucune réponse attendue (pause de 100 ms après 0x0F) | — |

## 4. Commandes

Colonne « Site → AK980 MAX » : ce que le site envoie réellement à ce modèle (d'après sa configuration : `routesName` = `customKeys, lighting, macro, performance, advancedKeys, settings`, sans `customKeysConfig`, `lightingConfig` ni `macroConfig`). Les commandes de la liste d'interdiction du plan (mise à jour, flash, écran, animations) sont marquées **interdite**.

| Code | Nom | Site → AK980 MAX | Taille / remarque |
|---|---|---|---|
| 0x01 / 0x02 | COMMUNICATION_START / END | jamais (déclarées seulement) | — |
| 0x0F | SET_FACTORY_RESET | oui : type 255 (Réglages) et 5 (calibrage) | §6.10 |
| 0x10 | GET_DEVICE_INFO | oui, à la connexion | 48, §6.1 |
| 0x11 / 0x21 | GET / SET_GAME_MODE | oui | 56, §6.2 |
| 0x12 / 0x22 | GET / SET_KEY | oui (+ lecture d'une touche) | 512, §6.3 |
| 0x13 / 0x23 | GET / SET_LED_EFFECT | oui | 16, §6.4 |
| 0x14 / 0x24 | GET / SET_CUSTOM_LED_DATA | oui | 512, §6.5 |
| 0x15 / 0x25 | GET / SET_MACRO | oui | 400 + données, §6.6 |
| 0x16 / 0x26 | GET / SET_FN_KEY | **non** (pas de `customKeysConfig.isShowFn`) | 512, même format que 0x12 |
| 0x17 / 0x27 | GET / SET_MAGNETIC_AXIS_RT | oui | 1024, §6.7 |
| 0x18 / 0x28 | GET / SET_MAGNETIC_AXIS_DKS_DATA | oui | 1024, §6.8 |
| 0x1B / 0x2B | GET / SET_LIGHT_BOX | non (autres modèles) | 24 |
| 0x1C | GET_DEFAULT_FN_KEY_MATRIX | **non** (seulement avec la couche Fn) | 512 |
| 0x1D / 0x2D | GET / SET_SIDE_LIGHT | non | 24 |
| 0x1E / 0x2E | GET / SET_TRIGGER_KEY | non (pas de page `trigger`) | 512 |
| 0x1F | GET_DEFAULT_KEY_MATRIX | **jamais** (déclarée, aucune fonction ne l'envoie) | — |
| 0x2A / 0x3D | SET / GET dot matrix | non (écran à points) | 10 |
| 0x30 / 0x31 | SET_KEYBOARD_CUSTOM_FUNCTION_ON / OFF | **jamais** (§6.13) | — |
| 0x32 | GET_LED_DATA | oui, **en écriture** : flux du mode musique | §6.12 |
| 0x33 | GET_ALL_LIGHTS_RGB | non (seulement si `lightingConfig.isEnableLightSync`) | 512, §6.15 |
| 0x34 | SET_TEMPORARY_COMMAND_DATA | non (écran) | — |
| 0x35 | SET_MUSIC_DATA | non (musique v1, 2.4 GHz) | §6.12 |
| 0x36, 0x37, 0x3B, 0x3C | CLEAR_LED_DATA, variantes 2.4 GHz | non | — |
| 0x40, 0x41, 0x42 | SET_LED_BOOT_ANIMATION, SET_LED_USER_ANIMATION, SET_LED_DATA | non — **interdite** | interface 0xFF67 |
| 0x4F | SET_FLASH_DOWNLOAD | non — **interdite** | — |
| 0x50, 0x51 | SET_TFT_* | non — **interdite** | — |
| 0x60 | GET_MAGNETIC_AXIS_KEY_STATUS | **jamais** (déclarée, format inconnu) | §6.11 |
| 0x64 / 0x65 | SET_CALIBRATION_ON / OFF | oui si `frameVersion == 0` (assistant de calibrage) | §6.11 |
| 0x66 / 0x67 | SET_SIMULATION_TEST_ON / OFF | non en pratique (seulement en mode test, désactivé) | §6.11 |
| 0x68 | GET_MAGNETIC_AXIS_STATUS | oui si `frameVersion == 1` (calibrage v2) | §6.11 |
| 0x69 / 0x6A | SET_CALIBRATION_ON_V2 / OFF_V2 | oui si `frameVersion == 1` | §6.11 |
| 0x80–0x86 | OTA_* | mise à jour — **interdite** ; le site lit aussi le numéro de série par 0x84 à la connexion si `frameVersion == 1` (nous ne le faisons pas) | — |
| 0x90 | SET_PROFILE | oui si `maxProfile > 1` | §6.9 |
| 0xFA | GET_DEVICE_NOTIFY | rapport spontané (écouté seulement pour les modèles à écran) | §6.14 |
| 0xFB | GET_MAGNETIC_AXIS_CALIBRATION_DATA | rapport spontané pendant le calibrage v1 / le test | §6.11 |
| 0xFC | GET_24G_DISCONNECT_NOTIFY | rapport spontané ; `[2] = 5` (remise à zéro) écouté pour tous les modèles | §6.14 |

## 5. Ce que le site lit pour ce modèle, et dans quel ordre

À la connexion (fonction d'initialisation du module protocole) :

1. `GET_DEVICE_INFO` 0x10, 48 octets. Si `firmwareStatus == 1` (clavier en attente de mise à jour), le site s'arrête là.
2. `GET_GAME_MODE` 0x11, 56 octets.
3. `GET_KEY` 0x12, 512 octets.
4. (Couche Fn : seulement si `customKeysConfig.isShowFn` — **pas pour l'AK980 MAX**, donc ni 0x16 ni 0x1C.)
5. Page `lighting` : `GET_LED_EFFECT` 0x13 (16), puis `GET_CUSTOM_LED_DATA` 0x14 (512). (Light box et éclairage latéral : non.)
6. Page `macro` : `GET_MACRO` 0x15 (table de 400 octets, puis chaque macro, §6.6).
7. Page `performance` : `GET_MAGNETIC_AXIS_RT` 0x17 (1024).
8. Page `advancedKeys` avec DKS : `GET_MAGNETIC_AXIS_DKS_DATA` 0x18 (1024).
9. Si `frameVersion == 1` : numéro de série par `OTA_GET_DEVICE_SN` 0x84 (interdite chez nous ; le numéro de série USB suffit).

Ensuite le site écoute les rapports `55 FC 05` (remise à zéro faite par le clavier) et relit tout s'il en reçoit un.

Écritures du site, par page :

- Touches : `SET_KEY` (512 octets, toute la couche) à chaque changement ; après avoir libéré une paire SOCD/RS, relecture des touches concernées une par une (§6.3).
- Touches avancées : MT et TGL → `SET_KEY` ; SOCD et RS → `SET_MAGNETIC_AXIS_RT` puis `SET_KEY` ; DKS → `SET_MAGNETIC_AXIS_DKS_DATA` puis `SET_KEY` ; suppression d'un DKS → `SET_KEY`, et l'entrée DKS remise à zéro puis la table réécrite.
- Éclairage : `SET_LED_EFFECT` (regroupé sur 300 ms, puis 500 ms de pause) et `SET_CUSTOM_LED_DATA` (regroupé sur 300 ms) ; mode musique : flux 0x32.
- Macros : `SET_MACRO` (table puis données).
- Performance : `SET_MAGNETIC_AXIS_RT` (regroupé sur 300 ms) ; zones mortes → `SET_GAME_MODE`.
- Réglages : `SET_GAME_MODE` (regroupé sur 300 ms) ; « tout réinitialiser » → `SET_FACTORY_RESET` 255.
- Profils de l'appli (côté navigateur) : application dans l'ordre touches, (couche Fn), effet, couleurs personnalisées, macros, magnétique, DKS. `GAME_MODE` n'en fait pas partie.

## 6. Blocs

Entiers multi-octets en petit-boutiste sauf mention contraire. « Non lu » : le site ignore l'octet en lecture ; « écrit 0 » : il l'écrit à zéro.

### 6.1 DeviceInfo — `GET_DEVICE_INFO` 0x10, 48 octets (lecture seule)

| Octets | Champ | Sens |
|---|---|---|
| 0 | `romSize` | |
| 1 | — | non lu |
| 2–3 | `macroSpaceSize` u16 | capacité des macros annoncée ; le site ne l'utilise que si `macroConfig.isGetMaxMacroSpaceSize`, sinon il prend 512 — **ce qui est le cas pour l'AK980 MAX** (§6.6) |
| 4–5 | `vid` u16 | |
| 6–7 | `pid` u16 | |
| 8–9 | `version` | `((b8 & 0x0F) + (b8 >> 4) × 10 + b9 × 100) / 100`, affiché avec 2 décimales (ex. `b8 = 0x23, b9 = 1` → « 1.23 ») |
| 10–11 | `sensor` u16 | |
| 12–13 | `manufacturer` u16 | |
| 14–15 | `product` u16 | |
| 16 | `workMode` | |
| 17 | `batteryLevel` | |
| 18 | `chargeStatus` | |
| 19 | `currentProfile` | numéroté à partir de 1 (le site utilise `currentProfile − 1` comme index) |
| 20–21 | `axisInfo` u16 | type de switch ; si non nul et qu'aucune entrée de `axisList` n'a ce `axisInfo`, le site renomme le switch par défaut d'après sa propre table de noms |
| 22–23 | `tftMaxFrames` u16 | |
| 24–25 | `gifMaxFrames` u16 | |
| 26–27 | `ledMaxFrames` u16 | |
| 28 | `tftDirection` | 255 = pas d'écran |
| 29 | `rtPrecision` | échelle des courses, §6.7 |
| 30 | `frameVersion` | 1 = délais longs, calibrage v2, `FUNC_V2` |
| 31 | `lightingVersion` | (variantes 2.4 GHz) |
| 32 | `firmwareStatus` | 1 = attente de mise à jour : ne rien lire d'autre |
| 33 | `maxProfile` | profils matériels si > 1 |
| 34–47 | — | non lus |

### 6.2 GameMode — `GET_GAME_MODE` 0x11 / `SET_GAME_MODE` 0x21, 56 octets

| Octet | Champ | Sens |
|---|---|---|
| 0 | — | non lu, écrit 0 |
| 1 | `gameMode` | |
| 2 | `fnSwitch` | |
| 3 | `sleepTime` | minutes, 0 = jamais (curseur 1–30 ; non proposé pour ce modèle) |
| 4 | `keyDelay` | 1–5 (non proposé pour ce modèle) |
| 5 | `reportRate` | code de fréquence, voir ci-dessous |
| 6 | `systemMode` | |
| 7 | `tftDisplayTime` | (écran) |
| 8 | `topDeadZone` | mm × 100 (zone morte haute) |
| 9 | `bottomDeadZone` | mm × 100 (zone morte basse) |
| 10 | — | non lu, écrit 0 |
| 11 | `stabilityMode` | 0/1 « Mode de stabilité » |
| 12–13 | — | non lus, écrits 0 |
| 14 | `autoCalibration` | 0/1 « Calibration dynamique adaptative (bêta) » |
| 15 | `singleKeyWakeup` | 0/1 (non proposé) |
| 16 | `pushButtonMode` | 0 volume, 1 luminosité du clavier, 2 luminosité de l'écran (non proposé) |
| 17 | `nkroSwitch` | (lu et réécrit tel quel, aucun réglage) |
| 18–19 | `wirelessReportRate` u16 | (2.4 GHz) |
| 20 | `powerMode` | (modèles « endurance ») |
| 21–55 | — | non lus, écrits 0 |

Codes de fréquence d'envoi (`reportRate`) : **3 = 1000 Hz, 4 = 2000 Hz, 5 = 4000 Hz, 6 = 8000 Hz** (table du site). Pour ce modèle le site propose 1000/4000/8000 Hz (codes 3, 5, 6) ; valeur supposée quand le champ manque : 6. Les codes 0–2 ne figurent nulle part (125/250/500 Hz probables — **non confirmé**). Le site prévient que changer la fréquence fait **ré-énumérer le clavier** sur l'USB : il faut s'attendre à une déconnexion puis reconnexion.

Réglages proposés par le site pour ce modèle (`settingsConfig`) : fréquence d'envoi, mode de stabilité, calibration adaptative ; zones mortes sur la page Performance (`isShowDeadZone`) : 0–0,50 mm par pas de 0,01, 0,30 mm à l'activation, 0/0 = désactivées.

Attention à l'écriture du site : il reconstruit les 56 octets à partir des seuls champs ci-dessus, donc **remet à zéro les octets non lus** (0, 10, 12–13, 21–55), et convertit `mm × 100` par troncature (0,29 mm → 28). Freejazz repart des 56 octets lus et arrondit.

### 6.3 Touches — `GET_KEY` 0x12 / `SET_KEY` 0x22 (et couche Fn 0x16 / 0x26), 512 octets

128 emplacements de 4 octets ; l'emplacement `i` (octets `4i … 4i+3`) est la touche d'index `i` (`index` de `layout.json`). Une action = `[page, p1, p2, p3]` :

| Page | Nom | Octets | Sens |
|---|---|---|---|
| 0x00 | DEFAULT | `00 00 00 00` | fonction d'origine de la touche. Le site écrit des zéros pour **toute** touche non réaffectée, et pour les emplacements sans touche. |
| 0x01 | MOUSE | `01 kind value 00` | `kind` 1 = bouton : `value` = 1 gauche, 2 droit, 4 milieu, 8 précédent, 16 suivant ; `kind` 3 = molette : `value` 1 haut, 255 bas (le site écrit `kind = 1` si absent) |
| 0x02 | KEYBOARD | écrit `02 00 usage 00` | `usage` = code HID (page clavier 0x07). En lecture, si `p1 ≠ 0`, c'est un masque de modificateur (1 Ctrl G, 2 Maj G, 4 Alt G, 8 Win G, 16 Ctrl D, 32 Maj D, 64 Alt D) que le site traduit en touche modificatrice ; sinon `p2` est le code HID. Le site écrit toujours les modificateurs comme codes HID 0xE0–0xE6 avec `p1 = 0`. |
| 0x03 | CONSUMER_KEY | `03 lo hi 00` | code multimédia u16 (page HID 0x0C), ex. 0x00E9 volume + |
| 0x04 | SYSTEM_KEY | ? | déclarée, ni lue ni écrite par le site |
| 0x05 | EXTRA_FUNCTION | ? | déclarée, ni lue ni écrite par le site |
| 0x06 | MACRO | `06 id mode count` | `id` 0–99 ; `mode` 0 « Général », 1 « Lecture répétée » (`count` = 1–255 répétitions), 2 « Terminer avec une nouvelle pression » ; `count = 0` sauf en mode 1 |
| 0x07 | CB | `07 p1 p2 p3` | combinaison de touches ; non proposée pour ce modèle, paramètres non décodés par le site |
| 0x08 | DKS | `08 slot 00 00` | `slot` 0–63 = entrée de la table DKS (§6.8) |
| 0x09 | MT | `09 maintien clic durée` | Mod-Tap (« Clic double ») : `p1` = code HID envoyé quand on **maintient**, `p2` = code HID envoyé sur un **clic**, `p3` = durée de maintien / 10 ms (10–1000 ms par pas de 10 → 1–100 ; 400 ms par défaut → 40) |
| 0x0A | TGL | `0A usage 00 00` | Toggle : un clic active/désactive le maintien continu de `usage`, un appui long se comporte normalement |
| 0x0B | SOCD | `0B mode usage1 usage2` | posée **sur les deux touches** de la paire ; `usage1`/`usage2` = codes HID (`hid` de `layout.json`) de la 1ʳᵉ et de la 2ᵉ touche choisies ; `mode` 3 « dernière entrée prioritaire » (défaut), 1 « touche 1 prioritaire », 2 « touche 2 prioritaire », 4 « annulation » (aucune des deux) |
| 0x0C | RS | `0C 00 usage1 usage2` | « Rappy Snappy » : posée sur les deux touches ; la plus enfoncée l'emporte |
| 0x0D | FUNC | `0D v>>16 v>>8 v` | fonction clavier, identifiant 24 bits **gros-boutiste** (`functions.json`, page `FUNC`) |
| 0x0E | END | `0E p1 p2 00` | « Appuyer/relâcher » ; non proposée pour ce modèle (sens de p1/p2 **non confirmé**) |
| 0x0F | MPT | `0F p1 p2 p3` | multi-course ; non proposée pour ce modèle |
| ≥ 0x80 | FUNC_V2 | `0x80 \| (a >> 8)`, `a & 0xFF`, `b >> 8`, `b & 0xFF` | deux valeurs : `a` (15 bits) et `b` (16 bits). Une valeur < 4096 est un code HID clavier, ≥ 4096 une fonction de la table `FUNC_V2` ; une fonction de `keyType` 2 va dans `a`, de `keyType` 1 dans `b` (sans doute appui long / appui court — **non confirmé**). Remplace `FUNC` quand `frameVersion == 1`. |

Autres faits :

- Pages proposées par le site pour l'AK980 MAX : touches clavier (listes `basic` et `extend`), souris et multimédia (`special`), macros, et les touches avancées `RS, SOCD, DKS, MT, TGL`. **Pas de fonctions `FUNC`** (elles ne sont offertes que sur la couche Fn ou si `isShowNormalFuncKeys`), **pas de couche Fn**.
- MT, TGL et DKS n'acceptent que des touches clavier (listes `basic` + `extend`, sans `Fn` = 175).
- SOCD et RS écrivent aussi le bloc magnétique (`SET_MAGNETIC_AXIS_RT`, §6.7) **avant** les touches (relu dans le bundle du site, composables `socd` / `rs`). RS : `triggerKeyStroke` des deux touches = celui de la **dernière** touche choisie (1,2 mm si 0), rien d'autre ne change. SOCD : les deux touches reçoivent, de la **première** touche choisie, `axisType`, `triggerKeyStroke` (1,2 mm si 0), `isWholeFast` et `isRampageMode` ; `pressRT` = celui de la première (0,1 mm si 0), `releaseRT` = le sien si `pressRT ≠ releaseRT` sinon `pressRT` ; les deux à 0 si le Rapid Trigger était désactivé ; les autres bits de drapeaux de chaque touche sont conservés. Réaffecter l'une des deux touches efface aussi l'autre (le site ne touche pas au bloc magnétique en les libérant).
- Touche « désactivée » : le site n'a **aucune** fonction pour cela (aucune entrée de valeur 0 dans ses listes de touches, aucun cas dédié dans son encodeur `lX`). Le seul octet qu'il produit pour « aucune touche » est celui d'une action KEYBOARD sans code : `case KEYBOARD: e[t]=2, e[t+2]=value||0` → `02 00 00 00`, que Freejazz écrit pour « Désactiver » ; à la relecture le site ne reconnaît pas cette action (avertissement « touche introuvable ») et l'affiche comme non réaffectée. Jamais essayé sur le clavier testé.
- Lecture d'une seule touche : en-tête sur mesure `AA 12 04 addr_lo addr_hi 00 00 00` (`addr = 4 × index`, sans drapeau de fin), réponse : 4 octets. Même chose avec 0x16 pour la couche Fn.
- Valeur renvoyée par `GET_KEY` pour une touche jamais réaffectée : `00 00 00 00` — **confirmé sur le clavier** (94 touches sur 101 ; les 7 autres sont des actions KEYBOARD, §9). Le site gère aussi `02 00 usage 00` : une action KEYBOARD égale au code HID de la touche compte comme « non modifiée ». Conséquence : `GET_KEY` ne permet pas de vérifier la correspondance index ↔ touche physique sans appuyer sur les touches.
- Le site perd les pages qu'il ne décode pas (0x04, 0x05, inconnues) : il réécrit 0 à leur place. Freejazz doit les conserver.
- « Par défaut » côté site = écrire `00 00 00 00` ; il n'utilise jamais `GET_DEFAULT_KEY_MATRIX` 0x1F.

### 6.4 Effet lumineux — `GET_LED_EFFECT` 0x13 / `SET_LED_EFFECT` 0x23, 16 octets

| Octet | Lecture | Écriture |
|---|---|---|
| 0 | `mode` : 0 = éclairage éteint, 1–19 effets intégrés, 128 = personnalisé (§6.5), 255 = GIF (autres modèles) | idem |
| 1–3 | couleur principale R, G, B ; **non fiable sur l'unité testée : relue `FF FF FF` après une écriture** (§9.7) | idem |
| 4 | `driverSetting` (le clavier testé renvoie 0, **même juste après une écriture de 255** — §9.7) | **255** |
| 5–7 | couleur secondaire R, G, B | idem (aucun effet de ce modèle ne la propose) |
| 8 | `colorMode` : 0 = couleur unique (« monochrome »), 1 = RVB (multicolore) | idem ; **non fiable en lecture sur l'unité testée : relu 1 (et la couleur `#ffffff`) après toute écriture, alors que le clavier affiche ce qui a été écrit** (§9.7) |
| 9 | `brightness` : 1–5 dans l'interface du site pour ce modèle (5 par défaut) ; **le clavier testé renvoie 6** (§9), et accepte l'écriture de 6 (§9.7) | idem |
| 10 | `speed` : 1–5 (3 par défaut) | idem |
| 11 | `direction` : 0 = →, 1 = ←, 2 = ↑, 3 = ↓ (selon l'effet) | idem |
| 12 | `effectModeType` (variantes, autres modèles) | idem |
| 13 | — non lu | 0 |
| 14 | `checkCodeL` (lu : 0, même après une écriture de 0xAA — §9.7) | **0xAA** |
| 15 | `checkCodeH` (lu : 0, même après une écriture de 0x55 — §9.7) | **0x55** |

- Liste des effets de ce modèle (aucun filtrage `lightingConfig`) : `effects.json` — mode, nom, si la couleur, la vitesse et la direction sont réglables, les `colorModes` proposés, la direction par défaut. En choisissant un effet, le site met `direction` à la direction par défaut de l'effet (0 si l'effet n'en a pas) ; en choisissant une couleur, il met `colorMode = 0`.
- Le site considère l'éclairage « allumé » si `mode` vaut 128 (ou 20/21, modes personnalisés d'autres modèles) ou figure dans la liste.
- Le site n'a pas d'aperçu continu pour ce modèle (`GET_ALL_LIGHTS_RGB` non utilisé, §6.15).

### 6.5 Couleurs personnalisées — `GET_CUSTOM_LED_DATA` 0x14 / `SET_CUSTOM_LED_DATA` 0x24, 512 octets

- 128 entrées de 4 octets `[index, R, G, B]`. En lecture, l'octet 0 est l'index renvoyé par le clavier ; en écriture, le site met **la position de l'entrée** (0–127) en octet 0, et l'entrée `i` est la LED de la touche d'index `i`. L'index lu est égal à la position pour les 128 entrées — **confirmé sur le clavier** (Freejazz range donc les entrées lues par position sans rien perdre).
- Effet visible quand `mode = 128` (§6.4) ; le site change d'abord le mode, puis envoie les couleurs (délai 2000 ms, regroupé sur 300 ms).
- Emplacements sans touche : le site réécrit ce qu'il a lu.

### 6.6 Macros — `GET_MACRO` 0x15 / `SET_MACRO` 0x25

Espace d'adresses des commandes 0x15/0x25 :

| Adresses | Contenu |
|---|---|
| 0–399 | table de 100 pointeurs u32 : adresse **absolue** de la macro `id`, 0 = pas de macro |
| 400… | les macros à la suite, dans l'ordre des `id` |

Une macro = en-tête de 4 octets `[n lo, n hi, 0, 0]` avec `n = 2 × nombre d'actions`, puis 4 octets par action :

| Octet | Sens |
|---|---|
| 0–1 | délai après l'action, u16 en ms (le site impose 5–60 000 ms, sauf 0 possible sur la dernière) |
| 2 | code : code HID pour une touche ; pour la souris, bouton 1 gauche, 2 droit, 4 milieu, 8 précédent, 16 suivant |
| 3 | drapeaux : bit 7 = appui (1) / relâchement (0) ; bits 4–6 = type (1 souris, 3 clavier) ; bits 0–3 = 0. Le site écrit `0x90`/`0x10` (souris, types 1 et 2) ou `0xB0`/`0x30` (clavier). |

- Lecture : table (400 octets depuis l'adresse 0 : 17 morceaux de 24, ou 8 de 56 sur l'unité testée, drapeau de fin sur le dernier), puis pour chaque pointeur `p ≠ 0` : 4 octets à l'adresse `p` (en-tête), puis `(n / 2) × 4` octets à l'adresse `p + 4`.
- Écriture : la table (400 octets, adresse 0, **aucun morceau n'a le drapeau de fin**), puis, s'il y a au moins une action, toutes les macros concaténées depuis l'adresse 400 avec le drapeau de fin sur le dernier morceau. Une macro sans action reçoit le pointeur 0.
- Capacité vérifiée par le site : `4 × nombre de macros + 4 × nombre total d'actions ≤ macroSpaceSize` et moins de 100 macros. Pour l'AK980 MAX, `macroSpaceSize` = **512** (valeur fixe du site ; la valeur des octets 2–3 de `GET_DEVICE_INFO` n'est pas utilisée : elle vaut **37376** (`00 92`, petit-boutiste comme le site la décode) sur l'unité testée, §9). La table de 400 octets n'entre pas dans ce compte.
- Les noms des macros ne sont pas dans le clavier (stockés côté site).
- Affectation à une touche : page 0x06 (§6.3).

### 6.7 Magnétique / Rapid Trigger — `GET_MAGNETIC_AXIS_RT` 0x17 / `SET_MAGNETIC_AXIS_RT` 0x27, 1024 octets

128 entrées de 8 octets (entrée `i` = touche d'index `i`) :

| Octets | Champ | Sens |
|---|---|---|
| 0 | `axisType` | type de switch : 1 `axis_9` « Switch magnétique riz », 2 `jade` « Axe Jade Pro », 3 `king` « Axe Magicien » (`model.json`) |
| 1 | drapeaux | bit 0 `isWholeFast` « Déclenchement rapide sur toute la course » (le Rapid Trigger reste actif sur toute la course au lieu de n'agir qu'au-delà du point de déclenchement) ; bit 1 `isRampageMode` « Mode rage » (masqué pour ce modèle) |
| 2–3 | `triggerKeyStroke` u16 | point de déclenchement : mm = brut / (`rtPrecision == 2` ? 1000 : 100) |
| 4–5 | `pressRT` u16 | sensibilité à l'appui : mm = brut / (`rtPrecision > 0` ? 1000 : 100) |
| 6–7 | `releaseRT` u16 | sensibilité au relâchement : même échelle que `pressRT` |

- Rapid Trigger désactivé ⇔ `pressRT = releaseRT = 0`. Sensibilités séparées ⇔ valeurs différentes.
- Écriture : `brut = round(mm × échelle)`.
- Limites de ce modèle (`model.json` → `performanceConfig`) : déclenchement 0,10–3,30 mm par pas de 0,01 (1,20 par défaut) ; sensibilité 0,01–3,30 mm pour les trois switchs (0,10 par défaut) ; le site déconseille moins de 0,04 mm.
- Changer de type de switch écrit `axisType` des touches choisies, puis le site demande de les recalibrer.

### 6.8 DKS — `GET_MAGNETIC_AXIS_DKS_DATA` 0x18 / `SET_MAGNETIC_AXIS_DKS_DATA` 0x28, 1024 octets

64 entrées de 16 octets (entrée `s` = `slot` de l'action `08 s 00 00`) :

| Octets | Champ | Sens |
|---|---|---|
| 0 | `makeValue1` | 1ᵉʳ point à l'appui, « point de départ de la touche », en 0,1 mm (16 = 1,6 mm par défaut) |
| 1 | `makeValue2` | 2ᵉ point à l'appui, « fond de la touche » (30 = 3,0 mm) |
| 2 | `breakValue1` | 1ᵉʳ point au relâchement, « relâcher depuis le fond » (30) |
| 3 | `breakValue2` | 2ᵉ point au relâchement, « fin du relâchement » (16) |
| 4, 6, 8, 10 | — | non lus, écrits 0 (octet haut d'une action sur 16 bits ? **non confirmé**) |
| 5, 7, 9, 11 | `action1…4` | code HID clavier de chaque action (0 = aucune) |
| 12–15 | étapes 1–4 | un octet par étape (appui au point 1, fond, relâchement depuis le fond, fin du relâchement) : bit `k` du quartet bas = l'action `k+1` est **déclenchée** à cette étape ; bit `k` du quartet haut = l'action `k+1` est **maintenue** pendant cette étape |

- Entrée vide ⇔ octets 0–3 nuls (le site la saute en lecture). Écriture : 1024 octets à zéro puis les entrées utilisées.
- Interface du site : `makeValue1 = breakValue2` et `makeValue2 = breakValue1` toujours, `makeValue1 < makeValue2`, `makeValue1 ≤ maxTriggerTrip × 10 − 1`, `makeValue2 ≥ 2`. Nouveau DKS = premier `slot` libre, première action = la touche elle-même.

### 6.9 Profils — `SET_PROFILE` 0x90

- Requête à en-tête sur mesure `AA 90 n`, `n` = numéro du profil à partir de 1 ; réponse 0x90 attendue (2000 ms) ; succès si l'octet 2 de la réponse vaut `n`.
- Utilisée seulement si `maxProfile > 1`. Après le changement, le site relit tout (§5).

### 6.10 Remise à zéro — `SET_FACTORY_RESET` 0x0F

- Un rapport `AA 0F type 00 00 00 00 00 …` (type en octet 2), pas de réponse attendue, puis 100 ms de pause.
- Types : 1 touches, 2 éclairage, 4 macros, 5 calibrage, 255 tout.
- Le site n'envoie que **255** (Réglages → « tout réinitialiser », puis rechargement de la page 500 ms après) et **5** (assistant de calibrage → effacer le calibrage). Les types 1, 2, 4 ne sont jamais envoyés — **non confirmé — à vérifier sur le matériel**.

### 6.11 Calibrage et état des touches (0x64–0x6A, 0x68, 0xFB, 0x66/0x67, 0x60)

Calibrage v1 (`frameVersion == 0`) :

1. `AA 64 00 …` (un rapport), réponse 0x64 attendue en 1000 ms.
2. Le clavier envoie alors de lui-même des rapports `GET_MAGNETIC_AXIS_CALIBRATION_DATA` (0xFB) :

| Octets | Champ |
|---|---|
| 0–1 | `55 FB` |
| 2 | index de la touche |
| 3 | état (1 = touche calibrée, en v1) |
| 4–5 | valeur max u16 |
| 6–7 | valeur min u16 (`& 0x7FFF`) |
| 8–9 | valeur courante u16 |
| 10–11 | course `keyStroke` u16 (affichée par le site en `keyStroke / 100` mm) |
| 12–13 | course max `maxStroke` u16 (affichée `/ 10`, unité **non confirmée**) |

3. Fin : `AA 65 00 …`, sans réponse attendue. Effacer : remise à zéro type 5.

Calibrage v2 (`frameVersion == 1`) :

1. `SET_CALIBRATION_ON_V2` 0x69 : en-tête sur mesure `AA 69 00 00 00 00 00 00`, 24 octets de données nulles, délai 2000 ms.
2. Le site interroge en boucle `GET_MAGNETIC_AXIS_STATUS` 0x68 par lots de 7 touches (dans l'ordre de la disposition) : données = pour chaque touche 8 octets `[index, 0, 0, 0, 0, 0, 0, 0]` (56 octets → 3 morceaux de 24, 24, 8 avec des rapports de 32, un seul de 56 avec des rapports de 64), `addrStart = numéro_du_lot × (reportCount − 8)`, adresse vérifiée dans la réponse, drapeau de fin seulement sur le dernier morceau du dernier lot, délai 2000 ms. Réponse par touche (8 octets) : `[état, adc lo, adc hi, course lo, course hi, …]` ; course en mm = brut / (`rtPrecision == 2` ? 1000 : 100) ; état 2 = touche calibrée.
3. Fin : `SET_CALIBRATION_OFF_V2` 0x6A, même forme que 0x69.

Test de course (`SET_SIMULATION_TEST_ON` 0x66 / `OFF` 0x67) : même forme que 0x64/0x65, le clavier envoie des rapports 0xFB, le site affiche `keyStroke / 100` mm. N'est proposé qu'en mode test (désactivé dans le site publié) et si `frameVersion == 0` : **le site ne l'envoie pas en pratique**.

`GET_MAGNETIC_AXIS_KEY_STATUS` 0x60 : déclarée, **jamais utilisée**, format inconnu. Freejazz ne l'envoie pas.

Freejazz (tâche 6) : `Keyboard::read_key_status` / `read_key_depths` suivent exactement la boucle 0x68 du calibrage v2 du site (fonction `getMagneticAxisStatus` : lots de 7 touches dans l'ordre de `layout.json`, `addrStart = lot × (reportCount − 8)`, adresse vérifiée, drapeau de fin seulement sur le dernier lot, 2000 ms ; réponse `état, adc u16, course u16`). Sur l'unité testée : 15 lots de 7 (le dernier de 3 touches), un morceau de 56 octets par lot. Marqué `// UNVERIFIED-HW` : **jamais envoyé au clavier** (le site ne l'envoie pas à un `frameVersion` 0) ; `freejazz-cli depth-test` refuse sans `--unverified`. Réponse du clavier, cadence possible et comportement touche enfoncée : **à vérifier sur le matériel**.

Conséquence pour la « profondeur d'appui en direct » : le site n'a pas de lecture de profondeur hors calibrage. Les sources possibles sont 0x68 (format connu, mais le site ne l'envoie que pendant le calibrage v2) et les rapports 0xFB après 0x66 (format connu, jamais envoyé en pratique). Les deux sont des commandes que le site n'envoie pas à ce modèle en usage normal — **à vérifier sur le matériel**.

### 6.12 Flux d'éclairage du mode musique — `GET_LED_DATA` 0x32 (écriture)

Onglet « musique » (version 2 pour ce modèle, proposé seulement sous Windows) :

- Toutes les 50 ms, le site calcule une image et l'envoie (si un envoi est en cours, l'image suivante attend et seule la plus récente part).
- Image : pour chaque touche de la disposition, dans l'ordre des rangées de `keyList` (celui de `layout.json`), 4 octets `[index, R, G, B]` — soit **101 × 4 = 404 octets** pour ce modèle. Les emplacements sans touche ne sont pas envoyés (ce n'est donc pas une image fixe de 128 × 4 = 512 octets).
- Envoi : commande 0x32, **un transfert par morceau de `c = reportCount − 8` octets** (vérifié dans le code du site, fonction d'envoi musique v2 : `a = reportCount || 32`, puis en USB `t = a − 8`, `s = ⌈longueur / t⌉` morceaux, chacun envoyé par un appel de transfert avec `contentSize` = longueur du morceau, `addrStart = morceau × t`, données complétées de zéros jusqu'à `t`, délai `frameVersion == 1 ? 2000 : 50`, `otherHeader` = `[amp]` sur le dernier morceau, `[amp, 0]` sinon ; ce n'est **pas** une taille fixe de 24 — les 24 octets n'apparaissent qu'avec le `reportCount` par défaut de 32) : morceau `t` = `AA 32 len addr_lo addr_hi amp f 00` + `c` octets (complétés de zéros), `addr = c·t`, `len` = `c` sauf pour le dernier. Morceaux autres que le dernier : en-tête complémentaire `[amp, 0]` → octet 5 = `amp`, octet 6 = 0 ; dernier morceau : `[amp]` → octet 5 = `amp`, octet 6 = 1. Avec des rapports de 32 (`c = 24`) : 17 morceaux, le dernier de 20 octets (404 − 384). **Sur l'unité testée (rapports de 64, `c = 56`) : 8 morceaux, le dernier de 12 octets (404 − 392).**
- Chaque morceau attend une réponse 0x32 en 50 ms (2000 ms si `frameVersion == 1`), 3 relances ; au-delà le site lâche le clavier.
- Octet d'amplitude `amp` : calculé seulement pour les claviers à écran à points (`ledConfig`), `⌈niveau × max(rangées, colonnes)⌉` ; **pour l'AK980 MAX il vaut toujours 0**.
- Avant le flux, le site n'envoie rien de particulier (ni `SET_LED_EFFECT`, ni 0x30) ; en quittant l'onglet, il arrête simplement d'envoyer.
- **Mesuré sur l'unité testée (tâche 6, §9.8)** : le clavier renvoie chaque morceau tel quel en écho (`55 32 len addr …` + les mêmes données) ; un morceau ≈ 4 ms aller-retour, une image (8 morceaux) ≈ 31,5 ms. **Débit maximal : 31,8 images/s** ; **30 images/s tenues** pendant 10 s sans aucune relance, mais sans marge (p95 33,3 ms pour 33,3 ms disponibles). Le site, lui, envoie une image toutes les 50 ms (20 images/s), tenues aussi sans relance. Freejazz : 30 images/s au plus, 20–25 conseillées.
- Retour seul du clavier à son effet après l'arrêt du flux : **non déterminable par lecture**. `GET_LED_EFFECT` renvoie l'effet enregistré, identique avant, pendant 10 s après le flux et après la remise ; il ne dit pas ce que les LED affichent. `GET_ALL_LIGHTS_RGB` 0x33, qui le dirait, n'est pas envoyé par le site à ce modèle : non essayé. Freejazz ne compte donc pas sur un retour seul : **`end_live` renvoie l'effet enregistré par `SET_LED_EFFECT`**, pris dans l'instantané lu avant toute écriture (la couleur relue par `GET_LED_EFFECT` après une écriture n'est pas fiable, §9.7). Le retour seul (oui/non, délai) est **à observer sur le matériel**, à l'œil.
- Musique v1 (autres modèles) : `SET_MUSIC_DATA` 0x35, un rapport `AA 35 00 mode p q` + 21 niveaux de bandes + somme de contrôle (octet 31 = somme des octets 0–30). Pas pour ce modèle.

### 6.13 `SET_KEYBOARD_CUSTOM_FUNCTION_ON/OFF` 0x30 / 0x31

Déclarées dans la liste des commandes et **utilisées nulle part** dans le code du site, ni autour du flux musique ni ailleurs. Rôle **non confirmé** (le nom évoque un mode où l'hôte pilote l'éclairage) ; ne pas les envoyer sans pouvoir observer le clavier.

### 6.14 Rapports spontanés du clavier

À ignorer pendant l'attente d'une réponse (le tri par commande s'en charge) :

- `55 FC 05` : le clavier a été remis à zéro (par exemple par une combinaison de touches) ; le site relit alors tout. `55 FC 04` / `55 FC 06` : déconnexion / mise en veille 2.4 GHz ; `A6 FF 01` : réveil 2.4 GHz.
- `55 FA type …` (`GET_DEVICE_NOTIFY`) : état du périphérique, écouté seulement pour les modèles à écran.
- `55 FB …` : données de calibrage / test (§6.11).

### 6.15 Commandes d'autres modèles (pour mémoire)

- `GET_ALL_LIGHTS_RGB` 0x33 : 512 octets envoyés `[i, 0, 0, 0]` pour i = 0–127, réponse `[index, R, G, B] × 128` = couleurs affichées en ce moment ; le site l'interroge toutes les 10 ms pour son aperçu, mais seulement si `lightingConfig.isEnableLightSync` (pas ce modèle).
- `GET/SET_TRIGGER_KEY` 0x1E/0x2E : 512 octets, 4 par touche `[rapide 0/1, point de déclenchement, point de relâchement, type de switch]`.
- Light box 0x1B/0x2B et éclairage latéral 0x1D/0x2D : 24 octets `[mode, R, G, B, …, colorMode (8), luminosité (9), vitesse (10)]`.

## 7. Écarts avec la spec et le plan (à arbitrer)

- **Couche Fn et fonctions `FUNC`** : le site ne lit ni n'écrit la couche Fn de l'AK980 MAX (pas de `customKeysConfig`), et n'y propose aucune fonction `FUNC`. Lire 0x16 / 0x1C sur ce modèle serait une commande que le site n'envoie pas.
- **Profondeur en direct** : 0x60 n'est jamais utilisée ; voir §6.11 pour les alternatives (0x68, 0x66 + 0xFB), toutes hors usage normal du site.
- **Image du flux** : 404 octets (touches présentes, ordre de la disposition) et non 512 ; 8 morceaux de 56 octets sur l'unité testée (17 de 24 avec des rapports de 32), et non 22 ; amplitude 0. **Appliqué (tâche 6)** : `send_frame` garde sa signature (image de 128 couleurs, amplitude) mais n'envoie que les touches de la disposition ; `begin_live` n'envoie rien ; `end_live` renvoie l'effet enregistré.
- **Taille des rapports** : 64 octets sur l'unité testée, et non 32 (morceaux de 56). La tâche 2 supposait 32 et sondait toute page d'usage ≥ `0xFF00`, dont `0xFF67` (`MI_03`, sorties de 4096 octets) : corrigé à la tâche 4 avant tout envoi — Freejazz lit `reportCount` dans le descripteur et ne sonde que les pages d'usage du site, avec la première requête du site (`GET_DEVICE_INFO`, un morceau `AA 10 30 00 00 00 01 00`).
- **Couche Fn** : non lue au premier contact (commande que le site n'envoie pas à ce modèle) — **à vérifier sur le matériel**.
- **« Par défaut »** : le site écrit `00 00 00 00` ; il n'envoie jamais `GET_DEFAULT_KEY_MATRIX` 0x1F.
- **Remises à zéro partielles** (types 1, 2, 4) : jamais envoyées par le site.
- **`isWholeFast`** : réglage par touche « Rapid Trigger sur toute la course », pas « sur tout le clavier ».
- **Capacité des macros** : 512 octets (en-têtes + actions) selon le site, indépendamment de `macroSpaceSize` lu.
- **GameMode** : le site remet à zéro les octets non lus quand il écrit ; Freejazz les conserve.

## 8. Données extraites (`data/ak980max/`)

Produites par `node tools/extract-site-data.mjs <dossier assets du site> data/ak980max` (analyse syntaxique avec acorn, sans exécuter le code du site ; le script échoue si une valeur n'est pas littérale, si un index est absent, en double ou ≥ 128, ou si deux touches se chevauchent).

- `layout.json` : `{ pid, keys: [{ index, label, hid, code, x, y, w, h }] }` — 101 touches, index 0–110 (pas contigus : les autres emplacements n'ont pas de touche). Configuration choisie par `productId = 32835` (le nom « AK980 MAX » sert seulement de vérification). `label` = libellé du site, `hid` = code HID (`keyCode` ; `Fn` = 175), `code` = code DOM `KeyboardEvent.code`. Corrections : `Numpadsubtract` → `NumpadSubtract`, et `Fn` (« -1 » chez le site) → chaîne vide.
- Géométrie, en unités de touche (1u = une touche de 56 px + son espace de 4 px), coin haut-gauche : on refait le calcul « flexbox » du site — chaque rangée fait la largeur de la plus large (`w-fit` ; ici la rangée de `A`, 20,8u), l'espace libre va d'abord à la barre d'espace (`flex-1`), sinon aux marges `auto`, sinon se répartit entre les touches (`justify-between`) ; `w-N` = N × 4 px, `ml-N`/`mr-N` = marges ; `h-29` = touche de deux rangées ; `y` = numéro de rangée. Résultat : Échap 1u, retour arrière 2,07u, barre d'espace 6,47u, `Num +` et `Num Enter` 2 rangées de haut, `Num 0` 2u. Les rangées 1 et 4 étant 4 px plus étroites que la rangée 3, leurs touches sont décalées de moins de 0,07u, comme sur le site.
- `model.json` : configuration du site pour le PID 32835 sans sa liste de touches (`routesName`, `musicConfig`, `performanceConfig`, `advancedKeysConfig`, `settingsConfig`, …), avec en plus le nom français de chaque switch (`fr`).
- `keycodes.json` : `[{ page, name, value, param1?, fr? }]` — tables `basic`, `extend` (touches clavier ; `param1` = masque de modificateur) et `special` (souris : `param1` = 1 bouton / 3 molette ; multimédia) du site, dans cet ordre.
- `functions.json` : `[{ page, value, name, keyType?, fr? }]` — fonctions `FUNC` puis `FUNC_V2` (`keyType` pour `FUNC_V2`, §6.3). Non proposées par le site pour ce modèle.
- `effects.json` : `[{ mode, name, fr?, hasColor, colorModes, hasSpeed, defaultSpeed?, hasDirection, directions?, defaultDirection?, custom? }]` — effets intégrés dans l'ordre du site ; `colorModes` = valeurs de `colorMode` proposées (`[0, 1]`, ou vide quand l'effet n'a pas de couleur ou pour le mode personnalisé) ; `directions` = valeurs du champ `direction` proposées (`[1, 0]` ←/→ ou `[2, 3]` ↑/↓).
- `fr` partout : libellé français du site (traduction de son texte, ou pour les touches sa légende française/AZERTY quand il en a une).

## 9. Relevé sur l'unité testée (2026-10-24)

Premier contact, **en lecture seule**, avec `freejazz-cli` (tâche 4), AK980 Max PID `0x8043` en USB. N'ont été envoyées que les lectures que le site envoie à ce modèle à la connexion : `0x10`, `0x11`, `0x12`, `0x13`, `0x14`, `0x15`, `0x17`, `0x18`. Aucune écriture, ni couche Fn (`0x16`, `0x1C`), ni `0x60`/`0x68`/`0x64`–`0x6A`, ni `0x0F`. Dans le CLI, chaque commande passe par un garde qui refuse tout autre code avant l'envoi.

### 9.1 Interfaces HID

Énumération Windows et descripteurs reconstitués (`freejazz-cli devices`, rien n'est envoyé au clavier) :

| Interface | Page d'usage / usage | Rapports (octets) | Rôle |
|---|---|---|---|
| `MI_00` | `0x0001` / `0x06` | entrée 8, sortie 1 | clavier « boot » |
| `MI_01` Col01 | `0x000C` / `0x01` | n° 3 : entrée 2 | multimédia |
| `MI_01` Col02 | `0x0001` / `0x80` | n° 2 : entrée 1 | contrôle système |
| `MI_01` Col03 | `0x0001` / `0x06` | n° 1 : entrée 15 | clavier NKRO (120 bits) |
| `MI_01` Col04 | `0x0001` / `0x02` | n° 6 : entrée 4 | souris |
| `MI_02` | `0xFF68` / `0x61` | entrée 64, sortie 64 | **commandes** : la seule que le site ouvre |
| `MI_03` | `0xFF67` / `0x61` | sortie 4096, fonction 64 | grands transferts (animations, écran d'autres modèles) : jamais ouverte |

- `MI_02` répond à `GET_DEVICE_INFO`. Requête : `00` (numéro de rapport) + `AA 10 30 00 00 00 01 00` + 56 zéros (65 octets écrits) ; réponse : 64 octets `55 10 30 00 00 00 01 00` + 56 octets de données, dont les 48 premiers sont utiles.
- Taille de rapport : 64 (`reportCount` de la première sortie de `MI_02`), morceaux de 56 octets ; tous les rapports reçus font 64 octets (numéro de rapport retiré par hidapi).
- Pas de numéro de série USB (chaîne vide) : la sauvegarde automatique « par numéro de série » devra se rabattre sur le PID.
- Lecture complète (`backup`) : 70 requêtes, 70 réponses au premier essai, ni relance ni rapport parasite.

### 9.2 `GET_DEVICE_INFO`

Octets lus : `00 00 00 92 45 0C 43 80 28 01 00 00 66 01 04 15 11 10 00 00 0E 00`, puis 26 × `00`.

| Champ | Valeur |
|---|---|
| micrologiciel (`version`) | **1.28** (`b8 = 0x28`, `b9 = 1`) |
| `vid` / `pid` | `0x0C45` / `0x8043` |
| `rtPrecision` | **0** : courses en 0,01 mm (déclenchement et Rapid Trigger : brut / 100) |
| `frameVersion` | **0** : délais de 500 ms (1000 ms pour `SET_KEY`), calibrage v1 (`0x64`/`0x65`), pas de `FUNC_V2`, pas de lecture du n° de série par `0x84` |
| `firmwareStatus` | **0** (pas d'attente de mise à jour) |
| `maxProfile` | **0** : pas de profils matériels (`SET_PROFILE` jamais envoyé) |
| `ledMaxFrames` / `gifMaxFrames` / `tftMaxFrames` | 0 / 0 / 0 |
| `tftDirection` | 0 (le site ne retire la page écran que pour 255 ; sans effet ici, ce modèle n'a pas de page `tft`) |
| `macroSpaceSize` (octets 2–3) | **37376** (`00 92`) ; le site prend 512 pour ce modèle |
| `romSize`, `sensor` | 0, 0 |
| `manufacturer`, `product` | `0x0166`, `0x1504` |
| `workMode`, `batteryLevel`, `chargeStatus` | 17, 16, 0 |
| `currentProfile` | 0 |
| `axisInfo` | 14 (`0x000E`) |
| `lightingVersion` | 0 |
| octets 1 et 34–47 (non lus par le site) | 0 |

### 9.3 Blocs lus

- **GAME_MODE** : `00 00 00 01 00 06 00 00 00 00 00 01 00 00 01 00` + 40 × `00` → veille 1 min, fréquence code 6 = 8000 Hz, zones mortes 0 / 0 (désactivées), mode de stabilité 1, calibration adaptative 1, NKRO 0. Aucun des octets que le site ne lit pas n'est non nul.
- **Touches** (`GET_KEY`) : 94 touches sur 101 à `00 00 00 00`, 7 actions KEYBOARD `02 00 usage 00` (§9.5). Emplacements sans touche : tous nuls.
- **Effet lumineux** : `09 FF B0 B0 00 00 00 00 00 06 03 00 00 00 00 00` → mode 9 « Fontaine colorée », couleur `#ffb0b0`, couleur unique (`colorMode` 0), luminosité **6** (l'interface du site propose 1–5 : Freejazz doit accepter la valeur lue), vitesse 3, direction 0. Octets 4 et 14–15 lus à 0 (le site écrit `FF` et `AA 55`).
- **Couleurs personnalisées** : les 101 touches en `#ffffff`, les 27 emplacements sans touche à `00 00 00` ; **l'octet d'index vaut la position pour les 128 entrées**.
- **Macros** : table de 100 pointeurs entièrement nulle, aucune macro.
- **Magnétique** : 100 touches switch 1 (`axis_9`, « Switch magnétique riz »), déclenchement 1,50 mm, Rapid Trigger 0,10 / 0,10 mm, drapeaux 0 ; barre d'espace : déclenchement 3,00 mm, Rapid Trigger 0,10 / 0,10 mm. Emplacements sans touche : nuls.
- **DKS** : 1024 octets nuls, aucune entrée.
- Sauvegardes (hors dépôt, copiées aussi dans `%APPDATA%\Freejazz\backups\`) : `backups/ak980max-8043-20261024-224845.json` (première, dont `raw` est encore le ré-encodage de l'instantané) et `backups/ak980max-8043-20261024-231109.json` (même contenu typé, `raw` = octets tels que lus, ex. effet `09 FF B0 B0 00 00 00 00 00 06 03 00 00 00 00 00`).

### 9.4 Réécriture et sauvegarde

Ré-encoder les blocs lus (ce qu'une restauration enverrait) redonne exactement les octets lus pour GAME_MODE, touches, couleurs personnalisées, macros, magnétique et DKS. Seul l'effet lumineux diffère, sur ses constantes : octet 4 (lu `00`, écrit `FF`) et octets 14–15 (lus `00 00`, écrits `AA 55`), comme le site les écrit.

La sauvegarde garde les deux : l'instantané typé (`snapshot`) et chaque bloc **tel que lu** (`raw` : octets 4 et 13–15 de l'effet, octets d'index des couleurs, zone des macros telle que le clavier l'a rangée, lue par `macroReads`). À la lecture, Freejazz vérifie que `raw` se décode en `snapshot` (tout sauf ces constantes d'écriture et la disposition des macros) et refuse sinon un fichier abîmé ou modifié à la main. **Une restauration écrit l'instantané typé ré-encodé à la façon du site** (avec `FF`, `AA 55`, index = position, macros rangées depuis l'adresse 400), pas les octets de `raw` : le micrologiciel attend les constantes d'écriture du site.

### 9.5 Correspondance index ↔ touches

Le clavier renvoie `00 00 00 00` pour une touche non réaffectée : la vérification prévue (chaque touche lue `Keyboard{usage}` avec `usage` = `hid` de `layout.json`) est impossible sans appuyer sur les touches. Les 7 emplacements non nuls :

| index | touche (`layout.json`) | `hid` attendu | lu |
|---|---|---|---|
| 54 | H | `0x0B` | H `0x0B` (identique) |
| 99 | Print | `0x46` | Ins `0x49` |
| 103 | Insert | `0x49` | Print `0x46` |
| 106 | Del | `0x4C` | Home `0x4A` |
| 104 | Home | `0x4A` | PgUp `0x4B` |
| 105 | PgUp | `0x4B` | PgDn `0x4E` |
| 108 | PgDn | `0x4E` | Del `0x4C` |

Print et Insert sont échangées ; dans la colonne de droite (de haut en bas Del, Home, PgUp, PgDn sur le dessin du site), chaque touche envoie celle du dessous et PgDn envoie Del. Ce sont vraisemblablement des réaffectations faites par le propriétaire du clavier sur le site (le clavier ne garde que les touches modifiées, et H → H ressemble à un essai), mais en lecture seule rien ne les distingue d'une liste de touches fausse dans `layout.json` — **à vérifier sur le matériel** (le propriétaire a-t-il réaffecté ces touches ? les légendes de ses touches suivent-elles le dessin du site ?). `layout.json` n'a pas été modifié. **Mise à jour du 2026-10-25** : ces valeurs survivent à une remise à zéro. Ce sont des valeurs d'usine, et le libellé et le `hid` de `layout.json` pour ces index sont suspects (§9.7).

### 9.6 Non fait — à vérifier sur le matériel

- Couche Fn (`0x16`, `0x1C`) : le site ne la lit jamais pour ce modèle ; `freejazz-cli dump fn-keys` refuse sans `--unverified`.
- État des touches en direct (`0x60`, `0x68`), calibrage (`0x64`–`0x6A`), remises à zéro (`0x0F`) : non envoyés. `freejazz-cli depth-test --unverified --seconds 5` (0x68) est prêt : lectures/s, format de la réponse touches relâchées (zéro attendu), puis touches enfoncées — **à vérifier sur le matériel**.
- Retour seul de l'éclairage après l'arrêt du flux 0x32 (§6.12, §9.8) : à vérifier sur le matériel.

### 9.7 Écritures (2026-10-24, tâche 5) — test interrompu

Premier test d'écriture : `freejazz-cli --trace selftest-writes --yes --dir backups`, une seule fois. Pour chaque bloc, dans l'ordre game-mode, led, custom-leds, keys, magnetic, dks, macros, puis l'effet et les couleurs enchaînés : le clavier contient-il la sauvegarde, écriture de la même valeur, d'une valeur modifiée, puis de la sauvegarde. Chaque écriture est suivie d'une relecture complète (`read_snapshot`), comparée bloc par bloc comme le fait le chargement d'une sauvegarde (blocs typés ; constantes d'écriture et disposition des macros exclues). Au premier échec, toute la sauvegarde est réécrite (comme `restore`) et relue.

N'ont été envoyées que des lectures du §5 et les écritures `SET_GAME_MODE` 0x21, `SET_KEY` 0x22, `SET_LED_EFFECT` 0x23, `SET_CUSTOM_LED_DATA` 0x24, `SET_MACRO` 0x25, `SET_MAGNETIC_AXIS_RT` 0x27, `SET_MAGNETIC_AXIS_DKS_DATA` 0x28, toutes au format du site (le garde du CLI refuse tout autre code avant l'envoi). Bilan : 625 requêtes, 625 réponses portant la bonne commande, du premier coup ; aucune relance, aucune attente sans réponse, aucune déconnexion. Durée totale 4,5 s.

| Bloc | Clavier = sauvegarde | Même valeur | Modification | Restauration |
|---|---|---|---|---|
| game-mode | OK | OK | OK (mode de stabilité 1 → 0) | OK |
| led | OK | **FAIL** | non fait | non fait |
| custom-leds, keys, magnetic, dks, macros, effet + couleurs enchaînés | non faits | | | |

Constaté sur le clavier :

- **`SET_GAME_MODE`** : un rapport `AA 21 38 00 00 00 01 00` + les 56 octets, réponse en écho en 32 ms environ. La même valeur est relue identique. Le mode de stabilité (octet 11) passe à 0 puis revient à 1 : chaque fois relu tel qu'écrit, la fréquence d'envoi (code 6) inchangée. **Aucune ré-énumération USB** quand la fréquence ne change pas. Lectures : environ 4 ms par rapport (69 rapports en 270 ms pour une relecture complète).
- **`SET_LED_EFFECT`** : écrit `AA 23 10 00 00 00 01 00` + `09 FF B0 B0 FF 00 00 00 00 06 03 00 00 00 AA 55`. C'est l'encodage du site (fonction d'écriture de l'effet relue dans le code du site : octet 4 = 255, 13 = 0, 14–15 = `AA 55`) appliqué à l'effet lu. Le clavier répond en écho. Relu aussitôt : `09 FF FF FF 00 00 00 00 01 06 03 00 00 00 00 00`.
  - Gardés : le mode (9, « Fontaine colorée »), la luminosité (6), la vitesse (3) et la direction (0).
  - Changés par le clavier : la couleur, de `#ffb0b0` à **`#ffffff`**, et `colorMode`, de 0 à **1** (RVB).
  - La restauration complète a réécrit les mêmes 16 octets ; l'effet a été relu environ 2 s plus tard (57 rapports d'écriture entre les deux), avec le même résultat. Ce n'est donc pas un état passager.
- **Octets 4, 13, 14, 15 de l'effet** : relus `00 00 00 00` juste après une écriture de `FF 00 AA 55`. Le clavier ne renvoie pas ces constantes (il ne les garde pas, ou ne les rapporte pas). La comparaison typée, qui les exclut, est donc la bonne.
- **`SET_CUSTOM_LED_DATA` juste après `SET_LED_EFFECT`** (dans la restauration, sans pause) : les 10 morceaux sont acquittés sans délai. Les couleurs relues sont égales à la sauvegarde, mais c'étaient déjà les valeurs du clavier. Le besoin d'une pause entre 0x23 et 0x24 n'est donc **ni confirmé ni écarté** : le test enchaîné n'a pas été atteint. Le site, lui, n'en met aucune quand il applique un profil (0x23 puis 0x24 directement) ; sa pause de 500 ms ne sépare que deux envois successifs de l'effet sur la page Éclairage.

Pourquoi la couleur relue change : diagnostic limité du 2026-10-25 (après accord). Seuls `GET_LED_EFFECT` 0x13 et 4 `SET_LED_EFFECT` 0x23 ont été envoyés, sans la sonde 0x10. Chaque écriture est un paquet que la page Éclairage du site peut envoyer, sauf la dernière, le paquet d'origine. Chacune a été relue aussitôt, puis 1 s après :

| # | Écrit (octets 0–15) | Relu |
|---|---|---|
| — | (état de départ) | `09 FF FF FF 00 00 00 00 01 06 03 00 00 00 00 00` |
| 1 | `09 FF B0 B0 FF 00 00 00 00 05 03 00 00 00 AA 55` (effet 9, couleur unique, luminosité 5) | `09 FF FF FF 00 00 00 00 01 05 03 00 …` |
| 2 | `09 FF B0 B0 FF 00 00 00 01 05 03 00 00 00 AA 55` (colorMode 1) | `09 FF FF FF 00 00 00 00 01 05 03 00 …` |
| 3 | `01 FF B0 B0 FF 00 00 00 00 05 03 00 00 00 AA 55` (effet 1 « Statique », couleur unique) | `01 FF FF FF 00 00 00 00 01 05 03 00 …` |
| 4 | `09 FF B0 B0 FF 00 00 00 00 06 03 00 00 00 AA 55` (paquet d'origine) | `09 FF FF FF 00 00 00 00 01 06 03 00 …` |

Chaque écriture est acquittée en écho en ~33 ms.

**Observation du propriétaire du clavier (2026-10-25)** : l'éclairage réel du clavier **n'a jamais changé** pendant ces écritures ni pendant le test. Il est resté sur la couleur unique rose. Le site officiel, fermé puis rouvert, lit lui aussi `#ffffff` / RVB.

**Conclusion (confirmé sur le clavier, micrologiciel 1.28)** :
- Les écritures de l'effet sont correctes : le clavier affiche ce qui est écrit, et le mode, la luminosité (6 acceptée) et la vitesse sont relus tels quels.
- Ce que `GET_LED_EFFECT` renvoie pour la couleur (octets 1–3) et `colorMode` (octet 8) **après une écriture n'est pas fiable** : `FF FF FF` et 1. La valeur lue avant toute écriture, rose et 0, était juste.
- Le site a le même problème de lecture.
- Ce n'est ni la luminosité 6, ni une inversion de `colorMode`, ni une particularité de l'effet 9.

Règle de Freejazz (décision de conception) :
- La relecture de l'effet compare le mode, la luminosité, la vitesse, la direction, la couleur secondaire et `effectModeType`.
- La couleur et `colorMode` sont **exclus**, signalés « non vérifiables par relecture » et non en échec (`LedEffect::matches_readback`).
- C'est la règle unique de `Snapshot::check_raw` : chargement d'une sauvegarde, `restore` et `selftest-writes`.
- Les sauvegardes gardent ce qui a été lu.

**État du clavier à l'arrêt** : identique à la sauvegarde d'avant le test (typée, comparée bloc par bloc). L'effet est relu `09 FF FF FF … 01 06 03 …`, mais il affiche toujours la couleur unique rose, d'après le propriétaire du clavier. Touches, couleurs personnalisées, macros, magnétique, DKS et GAME_MODE ont été réécrits par la restauration et relus égaux.

Fichiers (hors dépôt, chacun aussi dans `%APPDATA%\Freejazz\backups\`) :

- `backups/ak980max-8043-20261024-233609.json` : sauvegarde d'avant le test. Elle est identique (typée) à celle de la tâche 4 (`…-231109.json`).
- `backups/ak980max-8043-20261024-233613.json` : l'état relu après la restauration.
- `backups/ak980max-8043-20261025-091021.json` : l'état final après le diagnostic. C'est le fichier précédent, avec l'effet relu par 0x13 ; il est identique, et aucune autre lecture n'a été envoyée.

Conséquence pour Freejazz : après une écriture, l'appli ne peut pas relire la couleur de l'effet. Elle doit garder la couleur écrite (profil, sauvegarde) comme référence, au lieu de la reprendre d'une relecture.

#### Deuxième passage (2026-10-25, avec la règle de relecture de l'effet)

Même commande, une fois. Bilan : 1534 requêtes, toutes répondues du premier coup.

| Bloc | Clavier = sauvegarde | Même valeur | Modification | Restauration |
|---|---|---|---|---|
| game-mode | OK | OK | OK (stabilité 1 → 0) | OK |
| led | OK | OK | OK (luminosité 6 → 5) | OK |
| custom-leds | OK | OK | OK (index 0 → `#7c5cff`) | OK |
| keys | OK | OK | OK (F12 → volume +) | OK |
| magnetic | OK | OK | **FAIL : refusée avant envoi** | non fait |
| dks, macros, effet + couleurs enchaînés | non faits | | | |

- Après une écriture, l'effet est relu avec les octets 4, 13, 14 et 15 à `00` (comme au premier passage).
- Les index des couleurs personnalisées sont relus égaux à leur position.
- Écritures acquittées : 1 rapport en ~32 ms, 10 morceaux en ~179 ms (≈ 18 ms chacun), 19 morceaux en ~357 ms.
- Au FAIL, toute la sauvegarde a été réécrite, puis **vérifiée**. Le clavier est tel qu'il était au début de ce passage.

**Changement avant le passage : une remise à zéro faite par le propriétaire du clavier** (confirmée par lui). La sauvegarde prise au début du passage (`backups/ak980max-8043-20261025-093140.json`) diffère de celle de la veille :
- table magnétique entièrement nulle ;
- couleurs personnalisées nulles, octets d'index compris ;
- emplacement 54 (H → H) revenu à `00 00 00 00` ;
- **les 6 touches de navigation ont gardé leurs actions explicites**.

La modification magnétique partait d'une course de 0 (index 0 vide) et a été refusée par les limites avant tout envoi. Le test prend désormais l'index 0 si sa course est dans les limites, sinon la première touche de la disposition qui l'est (SKIP si aucune).

**Touches de navigation (décision de conception)** : après une remise à zéro, les 6 emplacements de §9.5 (99, 103, 104, 105, 106, 108) contiennent toujours des actions KEYBOARD explicites. Ce sont donc des **valeurs d'usine**, et non des réaffectations de l'utilisateur. Le libellé et le `hid` de `layout.json` pour ces index sont **suspects** : à trancher sur le matériel par le test des touches, plus tard. `layout.json` n'est pas modifié.

La sauvegarde a ensuite été restaurée : `…-20261024-233609.json` (`restore --yes`, tous les blocs relus identiques).

#### Troisième passage (2026-10-25) — tous les blocs vérifiés

Même commande, une fois. Bilan :
- 2499 requêtes, toutes répondues du premier coup ;
- seulement les lectures du §5 et 0x21–0x25, 0x27, 0x28 ;
- l'octet de fréquence à 6 dans chaque `SET_GAME_MODE` ;
- 13,9 s.

| Bloc | Clavier = sauvegarde | Même valeur | Modification | Restauration |
|---|---|---|---|---|
| game-mode | OK | OK | OK (stabilité 1 → 0) | OK |
| led | OK | OK | OK (luminosité 6 → 5) | OK |
| custom-leds | OK | OK | OK (Esc → `#7c5cff`) | OK |
| keys | OK | OK | OK (F12 → volume +) | OK |
| magnetic | OK | OK | OK (Esc 1,50 → 1,51 mm) | OK |
| dks | OK | OK | SKIP (table vide) | OK |
| macros | OK | OK | OK (macro 99, 2 actions) | OK |
| effet puis couleurs, sans pause | OK | OK | OK (luminosité 5 et Esc `#7c5cff`) | OK |

Confirmé sur le clavier :
- **Contrôle final :** la relecture complète est identique à la sauvegarde (`…-093531.json` → `…-093544.json`) : même JSON « snapshot », et aucun octet lu différent.
- **Pas de pause nécessaire entre `SET_LED_EFFECT` et `SET_CUSTOM_LED_DATA`.** Envoyés à la suite, les deux sont acquittés sans délai, et la couleur personnalisée modifiée est relue juste. L'écriture de la crate reste sans pause, comme le site quand il applique un profil.
- **Couleurs personnalisées :** contrairement à la couleur de l'effet, elles sont relues fidèlement. Les index sont relus égaux à leur position.
- **`SET_MACRO` :**
  - la table seule, sans drapeau de fin, est bien prise en compte : l'effacement de la macro 99 est relu ;
  - le clavier range la macro écrite comme le site : pointeur 400, en-tête `04 00 00 00`, actions `0A 00 04 B0`, `00 00 04 30`.
- **Temps d'écriture :** environ 18 ms par morceau acquitté (10 morceaux en 179 ms, 19 en 356 ms) ; lectures environ 4 ms par rapport. Aucune ré-énumération.

### 9.8 Flux d'éclairage en direct (2026-10-25, tâche 6)

Envoyés : `GET_DEVICE_INFO` 0x10, `GET_LED_EFFECT` 0x13, le flux `GET_LED_DATA` 0x32 et, une seule fois à la fin, `SET_LED_EFFECT` 0x23 (le garde de `stream-test` refuse tout autre code). Ni 0x30/0x31, ni 0x33, ni 0x60/0x68. Sauvegarde complète d'abord (`freejazz-cli backup --dir backups` → `backups/ak980max-8043-20261025-094903.json`).

- Format confirmé : 8 morceaux par image, `AA 32 38 addr 00 00 00` + 56 octets (adresses 0, 56, …, 336 ; `amp` = 0, octet 6 = 0), puis `AA 32 0C 88 01 00 01 00` + 12 octets (dernier morceau, octet 6 = 1). Chaque morceau est acquitté par l'écho exact de la requête (`55 32 …`, mêmes octets de données).
- Débit (`freejazz-cli stream-test`, arc-en-ciel défilant, binaire `--release`) :

| Cadence visée | Images en 10 s | Images/s | Envoi d'une image (min / moy. / p95 / max) | Relances |
|---|---|---|---|---|
| au plus vite (`--fps 0`) | 319 | **31,8** | 26,2 / 31,4 / 32,4 / 32,5 ms | 0 |
| 30 (`--fps 30`), deux fois | 300, 301 | **30,0** | 25,8 / 31,7 / 33,3 / 33,7 ms | 0 |
| 20 (`--fps 20`, la cadence du site) | 201 | 20,0 | 25,9 / 31,5 / 33,3 / 33,6 ms | 0 |

  Débit maximal tenu : **30 images/s**, sans marge ; 2552 + 2400 + 2408 + 1608 morceaux (plus 24 pour un premier essai tracé), tous acquittés au premier envoi.
- Après l'arrêt du flux (sans rien écrire), `GET_LED_EFFECT` lu toutes les 500 ms pendant 10 s : toujours `09 FF FF FF 00 00 00 00 01 06 03 00 …`, la valeur d'avant le flux. Cette lecture renvoie l'effet enregistré, pas ce qu'affichent les LED : elle ne dit pas si le clavier est revenu seul à son effet. **Retour seul : non déterminé** (§6.12).
- Remise : `end_live` avec l'effet de `backups/ak980max-8043-20261024-231109.json` (lu avant toute écriture : mode 9, `#ffb0b0`, couleur unique, luminosité 6, vitesse 3, direction 0) : un `SET_LED_EFFECT` `09 FF B0 B0 FF 00 00 00 00 06 03 00 00 00 AA 55`, acquitté ; relu mode, luminosité, vitesse, direction identiques (couleur et `colorMode` non vérifiables, §9.7). C'est le paquet d'origine du §9.7, que le clavier affiche en rose. **L'éclairage est à regarder sur le matériel** pour confirmer qu'il est bien revenu au rose.
