# Changelog

## 0.8.0

### Minor Changes

- Stabilize encoding and fix several issues related to time travel

### Patch Changes

- Updated dependencies
  - loro-wasm@0.8.0

## 0.7.2-alpha.4

### Patch Changes

- Fix encoding value err
- Updated dependencies
  - loro-wasm@0.7.2

## 0.7.2-alpha.3

### Patch Changes

- Fix export compressed snapshot
- Updated dependencies
  - loro-wasm@0.7.2

## 0.7.2-alpha.2

### Patch Changes

- Add compressed method
- Updated dependencies
  - loro-wasm@0.7.2

## 0.7.2-alpha.1

### Patch Changes

- Fix v0 exports
- Updated dependencies
  - loro-wasm@0.7.2

## 0.7.2-alpha.0

### Patch Changes

- Add experimental encode methods
- Updated dependencies
  - loro-wasm@0.7.2

## 0.7.1

### Patch Changes

- Fix a few richtext errors
- Updated dependencies
  - loro-wasm@0.7.1

## 0.7.0

### Minor Changes

- refactor: remove setPanicHook and call it internally when loaded

### Patch Changes

- Updated dependencies
  - loro-wasm@0.7.0

## 0.6.5

### Patch Changes

- Fix checkout err on seq data
- Updated dependencies
  - loro-wasm@0.6.5

## 0.6.4

### Patch Changes

- Fix time travel issue #211
- Updated dependencies
  - loro-wasm@0.6.4

## 0.6.3

### Patch Changes

- Fix isContainer issue

## 0.6.2

### Patch Changes

- Refine getType and isContainer

## 0.6.1

### Patch Changes

- 6753c2f: Refine loro-crdt api
- Updated dependencies [6753c2f]
  - loro-wasm@0.6.1

## 0.6.0

### Minor Changes

- Improve API of event

### Patch Changes

- Updated dependencies
  - loro-wasm@0.6.0

All notable changes to this project will be documented in this file. See [standard-version](https://github.com/conventional-changelog/standard-version) for commit guidelines.

## [0.5.0](https://github.com/loro-dev/loro/compare/v0.4.3...v0.5.0) (2023-11-27)

### ⚠ BREAKING CHANGES

- encoding schema is changed

### Bug Fixes

- [#181](https://github.com/loro-dev/loro/issues/181) importing should use inherent arena ([8e901cf](https://github.com/loro-dev/loro/commit/8e901cf00cc8469da136f18f13d2affc78e08e64))
- deno dirname in windows ([#183](https://github.com/loro-dev/loro/issues/183)) ([c04dc34](https://github.com/loro-dev/loro/commit/c04dc344f5413b5135354c9652a70b5d698f04ac))
- from snapshot should enable auto commit ([b940214](https://github.com/loro-dev/loro/commit/b94021498571cf7ac42f2896ca0abc82f15d823a))
- keep strong ref to doc in handlers [#190](https://github.com/loro-dev/loro/issues/190) ([#191](https://github.com/loro-dev/loro/issues/191)) ([e23ef43](https://github.com/loro-dev/loro/commit/e23ef4362d69430601728f40b730e72a183ac4ea))
- remove compress feature ([#184](https://github.com/loro-dev/loro/issues/184)) ([899270c](https://github.com/loro-dev/loro/commit/899270c6de065852d6e26a07b94b3d923cb83459))
- typo in lib.rs ([#176](https://github.com/loro-dev/loro/issues/176)) ([83b0e8c](https://github.com/loro-dev/loro/commit/83b0e8cc7f8bccd9d7c152c0e5a59437bebe6c87))

### [0.4.3](https://github.com/loro-dev/loro/compare/v0.4.2...v0.4.3) (2023-11-16)

### Bug Fixes

- avoid i32 overflow ([f799da9](https://github.com/loro-dev/loro/commit/f799da9abbdf369ea1f120c700078bb87f27212b))

### [0.4.2](https://github.com/loro-dev/loro/compare/v0.4.1...v0.4.2) (2023-11-16)

### Features

- get sub container directly when getting value ([#175](https://github.com/loro-dev/loro/issues/175)) ([1ff1505](https://github.com/loro-dev/loro/commit/1ff1505933198be8f3b7aefc32e1698007e21c25))
- **wasm:** add event id ([e54d2ac](https://github.com/loro-dev/loro/commit/e54d2ac21b02ad6b161336b0b9a99a0a85ca5a02))

### Bug Fixes

- loro-wasm typo ([#171](https://github.com/loro-dev/loro/issues/171)) ([c4b9cb4](https://github.com/loro-dev/loro/commit/c4b9cb4b2b77c3c83b99b51153cd8dd19a948dc0))

### 0.4.1 (2023-11-12)

### Features

- add `from_checkout` mark to event ([#164](https://github.com/loro-dev/loro/issues/164)) ([08e4ed9](https://github.com/loro-dev/loro/commit/08e4ed9d407128debe616cfb9d3865e1e69b338e))
- add bench ([d03617c](https://github.com/loro-dev/loro/commit/d03617ca265374f0e7be8ced81cb67611e2b1439))
- add compress ([c9f63b6](https://github.com/loro-dev/loro/commit/c9f63b6594dbb4e8ba4b059bf1d6e9bd37fade68))
- add compress ([6b6286d](https://github.com/loro-dev/loro/commit/6b6286d30b65a5ef6e49ff6b76e1234ae4c7ca25))
- add contains ([b30dee1](https://github.com/loro-dev/loro/commit/b30dee1386dbe2fd87534e34023b4e534b768790))
- add context check ([df3a708](https://github.com/loro-dev/loro/commit/df3a708e48f0c566ae516050922f17f153ac1d83))
- add cursor support ([59f59b1](https://github.com/loro-dev/loro/commit/59f59b1c2ebcee3130cd9af6701895a192c0d1a9))
- add decode state and loro to_json ([be02701](https://github.com/loro-dev/loro/commit/be0270140b0ac4d75551c2b4fe5558500257f523))
- add decode state and loro to_json ([92e2bff](https://github.com/loro-dev/loro/commit/92e2bff6adbca8845c072ebe964888a0a806ce1f))
- add delta compose ([9544e27](https://github.com/loro-dev/loro/commit/9544e27be4f9b769330a0fdba8f297e78d651c72))
- add deps to change ([3eb4157](https://github.com/loro-dev/loro/commit/3eb415718c1183ef9fcea67c11d3420642559610))
- add encode ([5969f92](https://github.com/loro-dev/loro/commit/5969f92b87a646e22d07e7b8393a5fb379d89688))
- add enum as inner fork ([7704ce2](https://github.com/loro-dev/loro/commit/7704ce2939b347f9acbf5cfb53ea0a765c5c862d))
- add fromCheckout to wasm ([fdd24bd](https://github.com/loro-dev/loro/commit/fdd24bd836a0466cccf90e605732fef9d88f9916))
- add gc feature gate ([811d585](https://github.com/loro-dev/loro/commit/811d585fed7e79ea2d818e0ae977863bd059f2e5))
- add go bindgen ([8f73917](https://github.com/loro-dev/loro/commit/8f739178e7210ec5eb08bd319be2fd4a4cc35b6c))
- add index map ([0ce9dbc](https://github.com/loro-dev/loro/commit/0ce9dbc3095b6b78acde92516e6811b3c90e3eea))
- add java bindgen ([271250b](https://github.com/loro-dev/loro/commit/271250ba05c3a2258fd42845dcbedb2bbcc5554f))
- add list notify ([4ed1eae](https://github.com/loro-dev/loro/commit/4ed1eaee32e90b0b92db66379d2f4189636f85c9))
- add map state snapshot ([48d784b](https://github.com/loro-dev/loro/commit/48d784bcd14562fb5dfdc92508eee2569e59a1c0))
- add notify to map and list ([c574e7e](https://github.com/loro-dev/loro/commit/c574e7ea5b6f000d7ee8cf0578d4a3dea8cce9a5))
- add origin to doc state diff ([874533e](https://github.com/loro-dev/loro/commit/874533e51a8f1f8810d0c513d74219126143fb79))
- add pool mapping ([1c8f378](https://github.com/loro-dev/loro/commit/1c8f3784f0a5e9b7f2cc9c5a6c4dd2e36e9c841c))
- add prelim ([bc8235f](https://github.com/loro-dev/loro/commit/bc8235ff47d6a862bc668648349376b3ec670fbd))
- add prelim struct ([43cb9cc](https://github.com/loro-dev/loro/commit/43cb9cc6db841be4f9e1bedcd0411698af15fa77))
- add Prelim struct ([b5880f0](https://github.com/loro-dev/loro/commit/b5880f0b667b03a1825671aa9790ae9d919816af))
- add push to List ([99278b1](https://github.com/loro-dev/loro/commit/99278b1e2294eee85bc60a69adbda3c91953cfa5))
- add python bingen ([8397274](https://github.com/loro-dev/loro/commit/83972746a6cee66c6e853f3e0460bb4f99006475))
- add recursive wasm ([4726677](https://github.com/loro-dev/loro/commit/47266773baa8923102b731561c0497c8b053d7ec))
- add return type for map and list ([39ece04](https://github.com/loro-dev/loro/commit/39ece045a8654e5d10a7b6097a28689755d29861))
- add rle global index tree trait ([067fb82](https://github.com/loro-dev/loro/commit/067fb82058ede7007e012fe040fb728ae87cc14e))
- add simple origin ([99e48b6](https://github.com/loro-dev/loro/commit/99e48b65ae11cafe2db913efb785c10deb9658a9))
- add subscribe to containers in wasm ([6df69bd](https://github.com/loro-dev/loro/commit/6df69bd2bec9460d27170df4fa83cd5753073e12))
- add tracing spans ([d718ed3](https://github.com/loro-dev/loro/commit/d718ed386fc93385d2911a45bcfb4afb2f353a32))
- add tracing spans ([32b53aa](https://github.com/loro-dev/loro/commit/32b53aaacb8e47f1dd876518356a196ae927d269))
- add typed versions of getMap and getList to Loro class ([#96](https://github.com/loro-dev/loro/issues/96)) ([bc09a04](https://github.com/loro-dev/loro/commit/bc09a0489f58bc99382edbfab3cb24eed9a0ccb0))
- add withstartend ([8f00518](https://github.com/loro-dev/loro/commit/8f005180a47e5a110deaf9626bbdf0c761db0d31))
- apply change ([8c0f033](https://github.com/loro-dev/loro/commit/8c0f033950da75b4be43b39ff1a7b6276b8fb643))
- autocommit transaction ([#127](https://github.com/loro-dev/loro/issues/127)) ([8293347](https://github.com/loro-dev/loro/commit/82933473341b235b07051131676cdadf79930b4d))
- basic import export pipeline ([f838373](https://github.com/loro-dev/loro/commit/f83837304e35fab7fbdd515fb3ad14cbd5139067))
- basic pipeline for text ([1f827f9](https://github.com/loro-dev/loro/commit/1f827f944ea71bdeab1385a8bfd717558aa60100))
- basic snapshot encoding ([e993f1b](https://github.com/loro-dev/loro/commit/e993f1b1558c8466af52e3435e18746a69875138))
- basic wasm interface ([e0a472f](https://github.com/loro-dev/loro/commit/e0a472fd1a50fd282f00b63571e0048ab4ad8632))
- change & op ([aae5cf2](https://github.com/loro-dev/loro/commit/aae5cf26ce3a1b07c57d0ea2babd4047b7d4b0c5))
- change cfg api ([b156023](https://github.com/loro-dev/loro/commit/b15602307ef2c9d2149ce076245bf4e1aa34261f))
- checkout to target version & use unicode index by default ([#98](https://github.com/loro-dev/loro/issues/98)) ([c105ff2](https://github.com/loro-dev/loro/commit/c105ff2220a3d9b98382d167d424422f15ad2bd3))
- cmp vv ([aa060a9](https://github.com/loro-dev/loro/commit/aa060a93da4519a6e3d9d703f629fe9323ef4df3))
- compact bytes init ([8704d22](https://github.com/loro-dev/loro/commit/8704d227508f448ccd06ffd011aa0aa7f71abf1b))
- compressed rle update encode mode ([#107](https://github.com/loro-dev/loro/issues/107)) ([17d1a9e](https://github.com/loro-dev/loro/commit/17d1a9ea829820438da618186dcc1f97a015c7d4))
- conenct container and store ([19c1215](https://github.com/loro-dev/loro/commit/19c12153f6006ec7f3e9cbf7a23c984cc85ccf20))
- connect diff calculator ([8ebd41f](https://github.com/loro-dev/loro/commit/8ebd41fa3d997926bd6213c15fe2ed4505c73b86))
- container checker ([0f2333b](https://github.com/loro-dev/loro/commit/0f2333b182214d6c2bb77948e4bb1f8dcf2ac5b4))
- container iter ([13becdb](https://github.com/loro-dev/loro/commit/13becdb3f3ee3de22dcd0cd6d64cb9363487e1c4))
- convert event to js & add vitest ([49f664d](https://github.com/loro-dev/loro/commit/49f664dd8f242192feb6853c102b9df0184d8db7))
- convert frontiers to version vector ([336bd1e](https://github.com/loro-dev/loro/commit/336bd1e497f02458373cc8d4dfad357957d19533))
- convert remote change to local change in oplog ([8f6a6e1](https://github.com/loro-dev/loro/commit/8f6a6e1cc2be03a9a3b42cf77a882f0d6c6db277))
- create doc from snapshot ([#136](https://github.com/loro-dev/loro/issues/136)) ([e1ab03f](https://github.com/loro-dev/loro/commit/e1ab03f30fd75b28d9465f6c85ed2eed650e4660))
- cursor mut ([66c50d4](https://github.com/loro-dev/loro/commit/66c50d4a9b33e61adca26f89e566c0149767a076))
- dag find path ([ed14536](https://github.com/loro-dev/loro/commit/ed145367e07caf2e2e9828944c982bc01cc8486a))
- dag init ([1ca2b42](https://github.com/loro-dev/loro/commit/1ca2b4226e201e956218d69c140a1151d1cb106c))
- dag iter ([b828783](https://github.com/loro-dev/loro/commit/b8287837dc57f86b787a288a812fe2a26c871f7f))
- dag partial iter ([8f6dd65](https://github.com/loro-dev/loro/commit/8f6dd6552259ed293721658ca693593b7ffae1f3))
- delete ([550bce2](https://github.com/loro-dev/loro/commit/550bce28153cc051864e84933c938593237b3570))
- delete range ([e19fb6a](https://github.com/loro-dev/loro/commit/e19fb6a91be10fffbffacd0291205cb7e03b2963))
- diff calc bring back & tree new event and value ([#149](https://github.com/loro-dev/loro/issues/149)) ([acafc76](https://github.com/loro-dev/loro/commit/acafc76aff71264df869c4aab7b5cd1b3519a185)), closes [#147](https://github.com/loro-dev/loro/issues/147) [#151](https://github.com/loro-dev/loro/issues/151)
- dynamic insert content ([efd806b](https://github.com/loro-dev/loro/commit/efd806b8e4f597ded735e9c01c7287ba9e0537c1))
- encode updates ([d3a0d10](https://github.com/loro-dev/loro/commit/d3a0d10b126a684b8e3d2719f0cbc70c19faee42))
- encode/decode v2 ([f208744](https://github.com/loro-dev/loro/commit/f208744ec1200e1ccb297e48a0bc7b09adf1a431))
- event (buggy) ([b5c325b](https://github.com/loro-dev/loro/commit/b5c325b4901d7c741517a69615e5d7157621b9ba))
- event & wasm ([15be521](https://github.com/loro-dev/loro/commit/15be521777e17004c4a60d3968b2f8e5978eba75))
- expose ContainerID ([cc129ee](https://github.com/loro-dev/loro/commit/cc129ee753933a8af2affda5185d2f5362e6156d))
- expose from loro crate ([490a54d](https://github.com/loro-dev/loro/commit/490a54d55936f2f2a1bee561df4009db8d39240b))
- expose frontier & make it comparable ([#95](https://github.com/loro-dev/loro/issues/95)) ([0a31b67](https://github.com/loro-dev/loro/commit/0a31b67dd4bad01262f9e06a08440ea6545fb7a0))
- expose version and change inspect api to wasm ([#156](https://github.com/loro-dev/loro/issues/156)) ([7ccfd1e](https://github.com/loro-dev/loro/commit/7ccfd1e91d3decece3e8e4937fb1260cd3c64309))
- extra pkg loro-crdt to wrap loro-wasm ([2f74b13](https://github.com/loro-dev/loro/commit/2f74b13e705548c30afc98db899df36d032e815f))
- fast gc ([794e001](https://github.com/loro-dev/loro/commit/794e001ce98e0026d5ceb013e6059c71a71d27c1))
- get missing span of vv ([b6d3f6b](https://github.com/loro-dev/loro/commit/b6d3f6b0b7f63d9ae5d777cd4bbead10b021fbeb))
- get op content from store ([4f2f07d](https://github.com/loro-dev/loro/commit/4f2f07dd32a7d699290fc17909a9679beb564aaa))
- get timestamp ([c31a4a0](https://github.com/loro-dev/loro/commit/c31a4a0239b2167a3fbb6a90915a4c64a3b29179))
- get value deep ([0d0603d](https://github.com/loro-dev/loro/commit/0d0603d75f85ec77217a6c9012355471689b5ffc))
- get vv from dag ([16395a4](https://github.com/loro-dev/loro/commit/16395a4fa291f103caea10d91489999272dcd49c))
- handlers ([a3488c7](https://github.com/loro-dev/loro/commit/a3488c708896cdf9366736eb7a4dedabbc04de42))
- hierarchy children & parent ([e402850](https://github.com/loro-dev/loro/commit/e4028504407d5f4f47cc6cf0d50f7d341547ca9d))
- impl C ffi ([95309db](https://github.com/loro-dev/loro/commit/95309db710873156f3650591dd033571ee07ffca))
- impl list map text transaction ([3c9818e](https://github.com/loro-dev/loro/commit/3c9818ef822d4773127abd0950f0c651726db07b))
- impl loro decode ([580f2e5](https://github.com/loro-dev/loro/commit/580f2e54beb5aa2e579b1a572cd29d07ea127d5d))
- impl yata ([670d194](https://github.com/loro-dev/loro/commit/670d194aeb7c08b741a74abec6666964a9f2f137))
- impl yata insert_at ([ec59679](https://github.com/loro-dev/loro/commit/ec596792f64b2e4969382ad2e83ad0085bb3d26f))
- implicit commit ([89c832e](https://github.com/loro-dev/loro/commit/89c832e2f2fb3c8c17f3779dab38b8be7e8fec70))
- import without state ([a8b7d65](https://github.com/loro-dev/loro/commit/a8b7d65f8a2962d0524778500df60ed5d7d407c7))
- init ([9ecd041](https://github.com/loro-dev/loro/commit/9ecd0417bdc262b3b39194e8f7b986b7310e841b))
- init content map ([bd1b0a2](https://github.com/loro-dev/loro/commit/bd1b0a22159c1a0eda750e49f1ff37ca78428925))
- init delta ([1ae9bf2](https://github.com/loro-dev/loro/commit/1ae9bf2a489f1b00d1632f2867921d295cda0ec7))
- init encoding and build pipeline for wasm ([f14905d](https://github.com/loro-dev/loro/commit/f14905d56276ab5041cb2d8ccd7021059535bd7e))
- init ffi ([a26d4b0](https://github.com/loro-dev/loro/commit/a26d4b0122f30b958b417ed92d43f823fa89adb8))
- init list container transaction ([47b9b34](https://github.com/loro-dev/loro/commit/47b9b348183c07feb5930d7d72378ace368014bd))
- init nodejs bindgen ([5b6f864](https://github.com/loro-dev/loro/commit/5b6f8644790f91894f99279d0d0ec9c2f89b443a))
- init txn ([bc11f0a](https://github.com/loro-dev/loro/commit/bc11f0a6d260f2e92b340b0e22623dd7b5cbc2a0))
- insert at cursor ([36c9fd7](https://github.com/loro-dev/loro/commit/36c9fd734046d41c133a2607b7cdb608d469fbca))
- insert obj to list ([b56d747](https://github.com/loro-dev/loro/commit/b56d747019fe1604da1238dc3869a134dee8e6af))
- insertion ([3c96a6b](https://github.com/loro-dev/loro/commit/3c96a6b224f15d85363662bf44e1c529b1794a43))
- integrate to text container ([dce9f03](https://github.com/loro-dev/loro/commit/dce9f0382176bc27f2f8143775d6d102af8329fa))
- introduce crdt-list ([cd95e22](https://github.com/loro-dev/loro/commit/cd95e2276cfc1e09a13ae67906c1c2b32fc75675))
- introduce rope ([c25500d](https://github.com/loro-dev/loro/commit/c25500df044c6af0912217b001bd8b0ac263e515))
- iter update in rle tree ([bc980c5](https://github.com/loro-dev/loro/commit/bc980c5b02e08240eb71e3ac0af7f7b92313fa1b))
- list & text states ([2cbe214](https://github.com/loro-dev/loro/commit/2cbe21463cd6b24b312bc6da7eafd0d052f0738f))
- list container ([077d696](https://github.com/loro-dev/loro/commit/077d696952f458bca6e2f156882303f02f7ed49f))
- list diff calculator ([d2c3eea](https://github.com/loro-dev/loro/commit/d2c3eead90585850180ecd115cf2638ff3597b43))
- list init ([29c4d20](https://github.com/loro-dev/loro/commit/29c4d2011e6853e708f1bde4a42055606048ffea))
- list push_front ([5743f8d](https://github.com/loro-dev/loro/commit/5743f8d989f728ddefae66307e55213b1295e1ec))
- loro use auto commit transaction ([47d1bb6](https://github.com/loro-dev/loro/commit/47d1bb603f6e3f09bd11f74e9f30f9dabd943a8e))
- LoroValue Binary ([331dc6c](https://github.com/loro-dev/loro/commit/331dc6c994aa53ee21b83c77847c51528b1d70a5))
- make capacity adjustable ([92434cc](https://github.com/loro-dev/loro/commit/92434ccdfc839ebe52cf3b87035d5b4178ca7bc1))
- make getting child container handler simple ([#104](https://github.com/loro-dev/loro/issues/104)) ([b22bd98](https://github.com/loro-dev/loro/commit/b22bd98f6b55e2e2661e708d623f9458515c7919))
- map basic ([a44a3cd](https://github.com/loro-dev/loro/commit/a44a3cd72b923845fdfc7b1385cfad989dbef534))
- map container ([aa9590b](https://github.com/loro-dev/loro/commit/aa9590b54017d4e8daa18072ee3a600fe58498c0))
- map transaction ([4652d83](https://github.com/loro-dev/loro/commit/4652d839ec1d32d2541f842cdfbd51460e5eecf9))
- mermaid ([77065bf](https://github.com/loro-dev/loro/commit/77065bf57ee3af4d44a063615fe9eda36d2829cb))
- **minor:** add a min match size ([e8ca8d6](https://github.com/loro-dev/loro/commit/e8ca8d61edfcf5807ef47fe6f1d724cd47d2d7bb))
- movable tree support ([#120](https://github.com/loro-dev/loro/issues/120)) ([e01e984](https://github.com/loro-dev/loro/commit/e01e98411c49e607a486a5478dc6a5a899ff6d9b))
- new map diff and map state ([4a8ce16](https://github.com/loro-dev/loro/commit/4a8ce16ff1e48119810c207932aec1688ac2dfba))
- new rle vec ([1c5cd94](https://github.com/loro-dev/loro/commit/1c5cd948edd447b412c0fa538bda2717ef2c56a5))
- notify ([72599b9](https://github.com/loro-dev/loro/commit/72599b99d1d612e5bdc10ee57c3a0818d4b4550c))
- op iter ([6c61c6b](https://github.com/loro-dev/loro/commit/6c61c6baf26280a60e057e7b2055497e2a9a9d9b))
- pending bk ([fdacd62](https://github.com/loro-dev/loro/commit/fdacd6282810d6c71c310b6f9952fe9625c2ec09))
- pending import ([c1a72c3](https://github.com/loro-dev/loro/commit/c1a72c3d7e2feb73258177226cb1d02d16385b17))
- pending remote changes, todo snapshot ([80a9d12](https://github.com/loro-dev/loro/commit/80a9d12ccc7d16e0699a9dfb05271a62f3746892))
- pending snapshot ([c34df16](https://github.com/loro-dev/loro/commit/c34df16dcb878c98558c6df3cf9958a80381837c))
- Peritext-like rich text support ([#123](https://github.com/loro-dev/loro/issues/123)) ([d942e3d](https://github.com/loro-dev/loro/commit/d942e3d7a2394509de0837faac3b453903d5d4e3))
- pin ([10bac8c](https://github.com/loro-dev/loro/commit/10bac8c2934ed6cf6803427a260e263034474fbf))
- readonly arena ([cc4e1d0](https://github.com/loro-dev/loro/commit/cc4e1d02e4b563fb35aab8a25526e6e8eac23ed9))
- record diff in app state ([7f3bd5b](https://github.com/loro-dev/loro/commit/7f3bd5b0a462bb6511521d9556f9da58a0e86962))
- record hierarchical info ([9bdb6b9](https://github.com/loro-dev/loro/commit/9bdb6b9fd4406a5715463db46a39d0a8b5493185))
- recursive emit events ([fbebb5b](https://github.com/loro-dev/loro/commit/fbebb5b8e86207cb78313dee253f1b60bddb2291))
- recursive map type; but perf becomes worse ([3d2ea64](https://github.com/loro-dev/loro/commit/3d2ea6479ac29d82c93046fd01a9a9cf86b38d4e))
- remove gc ([99c8529](https://github.com/loro-dev/loro/commit/99c852955987e855dd64242c45cea23f8d6aa6f3))
- replace notify set range method ([bd30f67](https://github.com/loro-dev/loro/commit/bd30f675a654de90b7d0369b3422476b64957130))
- return cursor in iter ([0d99ceb](https://github.com/loro-dev/loro/commit/0d99ceb01ce09b992c9abebf854722f5c33ba386))
- reuse tracker ([a1d1517](https://github.com/loro-dev/loro/commit/a1d1517de093a2feca12d29e6699c80f012355a7))
- rle ([2c7e2de](https://github.com/loro-dev/loro/commit/2c7e2de7639b14b3be1dbb8caeec21d4615bd792))
- rle tree insert ([028e3ba](https://github.com/loro-dev/loro/commit/028e3ba3f99adb2dcb73b838039040fee8924603))
- root subscriber & apply event to value ([aaf4e68](https://github.com/loro-dev/loro/commit/aaf4e6822b73fa8cfe8bf9c5f52bf5287f9a74cf))
- set range ([bf8973c](https://github.com/loro-dev/loro/commit/bf8973c7583e847a9855679fd235e5b835d250f0))
- setup framework ([fb27c16](https://github.com/loro-dev/loro/commit/fb27c1656b89d83084a2afe5345d4ab9f6166c2f))
- simple export and import ([5c47f2e](https://github.com/loro-dev/loro/commit/5c47f2e04e4315a7ac8405cb52530c84e62b67a0))
- state snapshot ([2fedf8d](https://github.com/loro-dev/loro/commit/2fedf8d3967a2c02dab21ddb7ab2429b2e2955dc))
- state snapshot import ([85865e5](https://github.com/loro-dev/loro/commit/85865e592ac699906dcacb38cbac78f1cfb9e720))
- subscribe for container events ([470d23a](https://github.com/loro-dev/loro/commit/470d23a1988a75cb43c1b2c8a9321a72c33c202c))
- subscribe unsubscribe ([e153f11](https://github.com/loro-dev/loro/commit/e153f113b8fe4823ddf7304a7a5b2ce10b04e895))
- supply-chain safety with cargo-vet ([1252bcd](https://github.com/loro-dev/loro/commit/1252bcdda99e17893d5dde6cfe47cbd09399f71b))
- support richtext in wasm & mark text with arbitrary value ([#142](https://github.com/loro-dev/loro/issues/142)) ([a40b5c6](https://github.com/loro-dev/loro/commit/a40b5c6e4a0a9a80e43d651f4596323182615069)), closes [#139](https://github.com/loro-dev/loro/issues/139)
- support txn abort for states ([fd588be](https://github.com/loro-dev/loro/commit/fd588beee27a41e00d3baf2d7529784237bc5a19))
- supports setting capacity ([346117f](https://github.com/loro-dev/loro/commit/346117ff5425876b5f5a870e306d4da5e60c3138))
- text transaction ([46e2c5a](https://github.com/loro-dev/loro/commit/46e2c5a960796420ea601351c321e96f555ebd33))
- text utf16 ([00dbf06](https://github.com/loro-dev/loro/commit/00dbf0622d5acb6163bd7c369cafdd4bd40798a0))
- to json and from json ([154ddfc](https://github.com/loro-dev/loro/commit/154ddfcfe56734647fdba8a75bc4e296d6e93015))
- transaction decode ([0ff122b](https://github.com/loro-dev/loro/commit/0ff122b68e7a063b5368e98542f89a0d13da7290))
- txn apply local op ([4634f0d](https://github.com/loro-dev/loro/commit/4634f0ddbb5cce07dafddb924c131795b73f2aa0))
- update at cursor pos ([374e323](https://github.com/loro-dev/loro/commit/374e32384ed56d6552d929bf9431e8bd1ece3971))
- update columnar ([5b0f3e3](https://github.com/loro-dev/loro/commit/5b0f3e3f50d9e13761a899bb103fcbdd9653cf93))
- use columnar iterable ([a1c3eea](https://github.com/loro-dev/loro/commit/a1c3eea4f12a981a1f9e0908a15ecde8d12de9db))
- use ContainerTrait ([9fefd75](https://github.com/loro-dev/loro/commit/9fefd75fb61bda94249b7f37d73cf12690590b45))
- use text tracker diff ([c50294a](https://github.com/loro-dev/loro/commit/c50294ac228df15146e9be9d148001683fd05b44))
- use the same api for container and temp container ([4bb3ea8](https://github.com/loro-dev/loro/commit/4bb3ea8b1b0c1f4019825ec9810f2dd25a1ecfaa))
- wasm encode decode basic ([91e7b3a](https://github.com/loro-dev/loro/commit/91e7b3ac87cabc700ccd9b4ac84eaa2991836fe0))
- wasm transaction ([ce00729](https://github.com/loro-dev/loro/commit/ce007295e13f5cb342411146a574977792a89b32))
- **wasm:** get deep value ([66d74c1](https://github.com/loro-dev/loro/commit/66d74c1e74c34bde619cfa455394e5e4c3cf5266))
- **wasm:** root subscribe & unsubscribe ([572fe85](https://github.com/loro-dev/loro/commit/572fe857a069abf2cff7eecdb4e71547d27a1f4b))

### Bug Fixes

- decode snapshot after pending ([4a1f4e8](https://github.com/loro-dev/loro/commit/4a1f4e86477a14d77973500ca1ca2feb2c30ca82))
- a few common ancestors bugs ([906aebf](https://github.com/loro-dev/loro/commit/906aebfa8abb53927ecaf4d0dd555adb0b8cb478))
- a few recursive bugs ([0fac770](https://github.com/loro-dev/loro/commit/0fac77030945495582b3492d7ecf0f99aa62e4b8))
- a weird deps bug ([b1d438d](https://github.com/loro-dev/loro/commit/b1d438d08d6e76bbeac01ff1175d295cda2c7e50))
- adapt crdt-list change ([a7ce6dd](https://github.com/loro-dev/loro/commit/a7ce6ddfd6515d219dbc7a9b9588ad8fcce1a291))
- add 2 site tests & fix update cursor bug ([b099b45](https://github.com/loro-dev/loro/commit/b099b4507c810045fe62981a00568ef60ea8e541))
- add compress ([3727fb7](https://github.com/loro-dev/loro/commit/3727fb7f7218a7ba79e8fe5f2d618702ee3ad56c))
- add debug info & reduce 40% mem usage ([1e9d576](https://github.com/loro-dev/loro/commit/1e9d5769f30a86df7608eae51d762dff01e076f4))
- add err when updates cannot be apply ([e4b6c5b](https://github.com/loro-dev/loro/commit/e4b6c5b96c17ca044ea71674954d91cf42bd8a8c))
- add event diff test & fix related bugs ([0a421d3](https://github.com/loro-dev/loro/commit/0a421d3931084af604130f0817bd2790e90cab45))
- add local info ([f9f556f](https://github.com/loro-dev/loro/commit/f9f556f822c65e64bf4988e0479c32d9ad1aa835))
- add root tracking test & and fix several related bugs ([06d53dd](https://github.com/loro-dev/loro/commit/06d53dd8a26a7db1050d26154950f01025e8a80f))
- add safety comment to rle ([9240ad1](https://github.com/loro-dev/loro/commit/9240ad12ee03033e7f20df4642461b3e9449205d))
- all vv and head vv error ([18235db](https://github.com/loro-dev/loro/commit/18235db95fb50b6b1e750ad77495546125949008))
- all_vv update ([8dbdf04](https://github.com/loro-dev/loro/commit/8dbdf04228de4f01616cf122c14c37eb9ae9f573))
- allow holes exist in tracker vv ([beeda6c](https://github.com/loro-dev/loro/commit/beeda6ccf6dda333315c096dd6b473a3cb05dbae))
- apply effects order ([16dd4c7](https://github.com/loro-dev/loro/commit/16dd4c7182b83b5b0958c5047a0144ac170801fa))
- avoid potential memory leak ([6a2da8a](https://github.com/loro-dev/loro/commit/6a2da8a01f9a8f82a63a6672f679471e848c68db))
- avoid repeatedly apply ([36adcd0](https://github.com/loro-dev/loro/commit/36adcd0ba302ad25723f09c152a265dbde2213dd))
- avoid Unresolved as PrelimValue ([a04d079](https://github.com/loro-dev/loro/commit/a04d0794aafc7173a2f7d53ecaf92c75047aec80))
- avoid zero len del in text ([b805661](https://github.com/loro-dev/loro/commit/b8056614f5a7f11a50630a72586dfbf7fbfcb916))
- basic import export test ([788808b](https://github.com/loro-dev/loro/commit/788808b05530f3f4e4d320094e8f9669e2786030))
- batch notify should be sorted by path length ([fb8a0e2](https://github.com/loro-dev/loro/commit/fb8a0e2e7b42e1aa8a0e1732269dd93bd42ee9d5))
- better capacity setting ([59d9c9b](https://github.com/loro-dev/loro/commit/59d9c9ba34b778362a52ea0e55d1182d0c407317))
- better dag find common ancestor ([e11e93f](https://github.com/loro-dev/loro/commit/e11e93fe07ec9033ca5212c742c3c0f785970dbc))
- bugs related to unknown type ([3d07e7e](https://github.com/loro-dev/loro/commit/3d07e7e7e5b3701b39f91a8366814d10de813634))
- build event when commit ([dd4e7d5](https://github.com/loro-dev/loro/commit/dd4e7d5ee89d33a1b38d7aebb3da84ee2e3d4054))
- build links between leaf nodes ([3fb88bd](https://github.com/loro-dev/loro/commit/3fb88bde6ee3321b3a4af1f85d4b23705346f758))
- cache error ([8807d43](https://github.com/loro-dev/loro/commit/8807d43eca9795401e08bbe96984aefadbf2c763))
- cache update in list diff calc ([5f5db10](https://github.com/loro-dev/loro/commit/5f5db10a6db50be3ff98594650614a72a4d901a2))
- calculate lamport by deps ([e418978](https://github.com/loro-dev/loro/commit/e4189785ea2d0ffa8cc65863e60f37b701da20e3))
- cap ([6546577](https://github.com/loro-dev/loro/commit/65465774efe6d8bcc9c67e1b5b21a9b69a1d3b6a))
- cargo fix ([50b2834](https://github.com/loro-dev/loro/commit/50b283493d316b3b6356ba1a9dff7dee11ee63ba))
- causal iter sort ([73bc9a7](https://github.com/loro-dev/loro/commit/73bc9a74f94deb0d48d7c5d930d539294bbe9b30))
- change deps bug ([6dcd9d1](https://github.com/loro-dev/loro/commit/6dcd9d19e842f2c95b67b047cfb2491dbdb41ba9))
- changes traveler bug ([3551bc4](https://github.com/loro-dev/loro/commit/3551bc4e99293961bc9d789fad86dc67cd5da241))
- check err ([6a8087c](https://github.com/loro-dev/loro/commit/6a8087c756946467bdca2d3866d74ebf22146375))
- checkout result err ([05f8023](https://github.com/loro-dev/loro/commit/05f802376eab01442a27e6c970a9618923d1a123))
- client idx use Rle ([e048224](https://github.com/loro-dev/loro/commit/e0482242639ee25f3264fa52fc0e75f701e9c285))
- columnar iter name ([0b64a56](https://github.com/loro-dev/loro/commit/0b64a567ed9fb6dc11df7764ce6e9dfea2c7a512))
- commit txn when dropping ([93a52ba](https://github.com/loro-dev/loro/commit/93a52ba55ed786664e302ed97400ef230a1d1093))
- common ancestor step 1 ([352ddc1](https://github.com/loro-dev/loro/commit/352ddc1c11a633cdbcffd6353b630f738d6e7fef))
- compress flag ([b3420e4](https://github.com/loro-dev/loro/commit/b3420e4f649bee7f89a4f396a3136bc34552b5b9))
- container id should be converted to js string ([f27786f](https://github.com/loro-dev/loro/commit/f27786fa2592b683cca9160f707b4ca2ed8a375a))
- container length is inconsistent when fuzzing caused by decode ([95ad837](https://github.com/loro-dev/loro/commit/95ad837a79ffde6ae92fb2f6abc1f218d9982408))
- container may be deleted from doc when editing ([bc66583](https://github.com/loro-dev/loro/commit/bc66583863ddc6076337fa195c52e7f7d3f3b81b))
- crdt-list yata integrate err ([4213d4c](https://github.com/loro-dev/loro/commit/4213d4c488b1f96c90f2ee66ba6109050ce19695))
- cursor get_sliced should have len > 0 ([9d31605](https://github.com/loro-dev/loro/commit/9d31605bde7013db38d8c173345f883404f0dd5b))
- cursor should be invalidated ([65d6f4f](https://github.com/loro-dev/loro/commit/65d6f4ffe90465fa66bd15cfcfc124a21f0050a5))
- cursor should not use Deref ([a93d9f7](https://github.com/loro-dev/loro/commit/a93d9f762d78dff472640ed54320d57766fcfa72))
- dag ([78faec3](https://github.com/loro-dev/loro/commit/78faec33a1b43499df9c6d3e19ba0ec2b6f35d6c))
- dag issues ([8db4778](https://github.com/loro-dev/loro/commit/8db47780b9ee5349b8c6c9db67dcd944317f6592))
- dag partial iter bug ([6473d9c](https://github.com/loro-dev/loro/commit/6473d9c11ef13950a96296ae388197dae87303bb))
- dead lock on list ([b94274d](https://github.com/loro-dev/loro/commit/b94274d8b983b178045563dab8a5c5c7cb03b3e3))
- decode batch ([#54](https://github.com/loro-dev/loro/issues/54)) ([625771c](https://github.com/loro-dev/loro/commit/625771c37da67ce30d707ac00a8cfee00ddf8173))
- decode deps ([45c1a2e](https://github.com/loro-dev/loro/commit/45c1a2e791d5c1de0bcbe4505f552149138a0334))
- decode hierarchy for snapshot mode ([4748e1d](https://github.com/loro-dev/loro/commit/4748e1d38c10c285dd055e2611ec04db26582776))
- decode notify ([27eb840](https://github.com/loro-dev/loro/commit/27eb84052503800262923fb3902e592e05f7fe3f))
- decode remove unknown ([eb8a076](https://github.com/loro-dev/loro/commit/eb8a07641f33b27da01f916fc4efb92a6c734f92))
- decode unknown ([89a2659](https://github.com/loro-dev/loro/commit/89a2659dfb036cf9aae9ecce30c0eb48e4430d7b))
- delete span iter bug ([ec4e192](https://github.com/loro-dev/loro/commit/ec4e1926cbf1c81f7797d087a950da3e26a52656))
- delta compose delete insert ([c4a62be](https://github.com/loro-dev/loro/commit/c4a62bee37bb9f3fde5080f2f3629fcd10975991))
- DeltaValue trait add length ([25c1f44](https://github.com/loro-dev/loro/commit/25c1f449be733d2f458ee6b6d6683922119f229f))
- deno tests ([e01b695](https://github.com/loro-dev/loro/commit/e01b6954dbc56f378df74117c591d722f3b4de49))
- dep counter ([b8e27dc](https://github.com/loro-dev/loro/commit/b8e27dc011f4b708ca913f5900b08fbc2464bd4c))
- dep is in merged pending change ([d4f786e](https://github.com/loro-dev/loro/commit/d4f786e64a1d758dcf51db618e8ad026bf8df826))
- diff calc err ([e15e207](https://github.com/loro-dev/loro/commit/e15e207cab86822336bc5708945c22bdbaec9142))
- effect iter, get_cursor_at_id_span bug ([b057201](https://github.com/loro-dev/loro/commit/b0572016a02e55756bd7e47a7e083c28077c9222))
- empty doc with pending decode snapshot ([7b012de](https://github.com/loro-dev/loro/commit/7b012dec0052f5001e28e219924a8b13679918e1))
- empty input ([8bb427a](https://github.com/loro-dev/loro/commit/8bb427a969eecfd888234dddbb288e04ff26f3e3))
- encode enhanced pending changes ([a77bf2f](https://github.com/loro-dev/loro/commit/a77bf2fcb3be552f789c547ec4e3491178e49de0))
- encode use dep_on_self ([5b22a1e](https://github.com/loro-dev/loro/commit/5b22a1e9aa12b4098e70a6d488355b88ed8ad2ae))
- encode when only create container but no op ([f468e3b](https://github.com/loro-dev/loro/commit/f468e3b57b89753b2861a9380bab7903b079c07b))
- encode_from no compression by default ([#77](https://github.com/loro-dev/loro/issues/77)) ([1002c9c](https://github.com/loro-dev/loro/commit/1002c9cca569d38c7c7de6f2e42e5b6257c188b5))
- encoding ([889f564](https://github.com/loro-dev/loro/commit/889f564779d9d8b2308a03a11d3b331f6a02f3a1))
- encoding error ([a91b43a](https://github.com/loro-dev/loro/commit/a91b43ab2523cc686f0cb7b6d217779ad3a57d68))
- encoding merge err ([3bb2d34](https://github.com/loro-dev/loro/commit/3bb2d3490ddb86faa638f6b96e8ebb32fd758d16))
- encoding version use u8 ([bfeed8f](https://github.com/loro-dev/loro/commit/bfeed8fb2ed96513ca8fcc4e518a7767c4f215a2))
- export iter bug ([985a8f6](https://github.com/loro-dev/loro/commit/985a8f6920749bb7c55a23b4e90377a76bba2370))
- feature err ([2ecb156](https://github.com/loro-dev/loro/commit/2ecb156f3077f702bbe211a7896a37cd86c3fd05))
- find yspan.origin right error ([5ac137c](https://github.com/loro-dev/loro/commit/5ac137c877a567cf357a62c040f31e46bf6d216b))
- find_path ([15c60ae](https://github.com/loro-dev/loro/commit/15c60aece0b5446cc45988416ec1f17f7238de36))
- first met dep may have smaller counter ([fec3c27](https://github.com/loro-dev/loro/commit/fec3c272f8494043fbd46f8139beae7c57e47cc9))
- fix a delete bug & init bench ([9e66e2d](https://github.com/loro-dev/loro/commit/9e66e2dc681c3294cde35656416649995a5e0623))
- fix a encode/decode issue ([3638e3d](https://github.com/loro-dev/loro/commit/3638e3d0ed2b321689243d97955eeb7571c0f465))
- fix a few bugs ([61c27ca](https://github.com/loro-dev/loro/commit/61c27ca58b8b0ef62cae6fabbb136f56b976ac64))
- fix a few bugs ([5f6d663](https://github.com/loro-dev/loro/commit/5f6d66368e2253926c4ff0eecdde5b35ba404d38))
- fix a few import panic ([1d3cd60](https://github.com/loro-dev/loro/commit/1d3cd60873af32bfee27a89a2c4607275c8cec9b))
- fix a few recursive_refactored bug ([16ec59d](https://github.com/loro-dev/loro/commit/16ec59ddee6b4436014a2b7343915b4b5df13de0))
- fix insertion err ([1f0f502](https://github.com/loro-dev/loro/commit/1f0f502be536ebc12a6f7966a6627339d9a6f35d))
- fix lamport infer in change encode ([f527de5](https://github.com/loro-dev/loro/commit/f527de5a2fbab77ed5cef8b4a118bedca411411c))
- fix memory leak ([b582e00](https://github.com/loro-dev/loro/commit/b582e005cb2f0cafc556389e3e8a753db9b936aa))
- fix several bugs ([bcc2c57](https://github.com/loro-dev/loro/commit/bcc2c5755601a225bb53d7b3c15add4f52940151))
- fix several issues detected by fuzzer ([886c1cd](https://github.com/loro-dev/loro/commit/886c1cdd4a246c34da49bdfebcf89f8fd8980784))
- fix several iter & delete bug ([5104e94](https://github.com/loro-dev/loro/commit/5104e94cd0e5943500f98971e197244ee2059b00))
- fix yspan merge bug ([d7d626d](https://github.com/loro-dev/loro/commit/d7d626dd97d251be7eac6f699e26cac38f3318d0))
- fuzz ([fba6024](https://github.com/loro-dev/loro/commit/fba6024754b2bcfffcda934280e0806520bae0d9))
- fuzz deps ([e85ba3f](https://github.com/loro-dev/loro/commit/e85ba3f3407693167117644184874089aab1929b))
- g-btree bug, fixed by upgrade dep ([a52549e](https://github.com/loro-dev/loro/commit/a52549ea309fa9bd581628c3b0268fac69ff123c))
- g-btree delete leaf err ([bcf81a4](https://github.com/loro-dev/loro/commit/bcf81a45eb23ceb35a96af6914bdc785ca612571))
- get container by id err ([b98c225](https://github.com/loro-dev/loro/commit/b98c22570d0c9bbc0307d7df4006b544a2379cac))
- get cursors at id span bug ([7154b5e](https://github.com/loro-dev/loro/commit/7154b5e8fe93b50e478083b240f5ed977cc39b25))
- get deep value & throw mismatched context err ([88003bd](https://github.com/loro-dev/loro/commit/88003bdffee01a5d41058e5044d8d9a6b8c77158))
- get first cursor at id span err ([46252a4](https://github.com/loro-dev/loro/commit/46252a4b4a4efa10f20f96f152c18b9d3ca089b7))
- get is span should consider unapplied status ([9080e68](https://github.com/loro-dev/loro/commit/9080e68c8991ad5b6ec32d74d70380822a47c752))
- get lamport by frontiers bug ([3d3c54e](https://github.com/loro-dev/loro/commit/3d3c54eb92eb78e77e5485e51f5cb2f89e4650d2))
- get path dead loop ([76f995f](https://github.com/loro-dev/loro/commit/76f995f48fd957c516818c6262e7e432d9bef6f0))
- hierarchy notify ([a24e284](https://github.com/loro-dev/loro/commit/a24e284fbae519766fdb12eadced0ea87dfa527b))
- impl Fugue correctly ([#133](https://github.com/loro-dev/loro/issues/133)) ([c9cf106](https://github.com/loro-dev/loro/commit/c9cf106338d0c4c6f94181156f0be0d303e1dcb6))
- import ([f0b8cf3](https://github.com/loro-dev/loro/commit/f0b8cf301fbecd9d98f79ad1f762ba578a08f517))
- import change slice ([e6a4be5](https://github.com/loro-dev/loro/commit/e6a4be5dcf60a6d37710f87f85e4f24cdab063aa))
- import context diff should keep causal order ([63bb791](https://github.com/loro-dev/loro/commit/63bb791abae65d6539376976cdf13f525eadae4d))
- imported changes were not mergeable ([#147](https://github.com/loro-dev/loro/issues/147)) ([5b65963](https://github.com/loro-dev/loro/commit/5b65963104da368ac48f8e83f1583958137fe28e))
- insert / delete 0 length content ([31b0fa3](https://github.com/loro-dev/loro/commit/31b0fa335462f56934a7965f6ec7bcc34683a1c3))
- insert map logic bug ([d34abee](https://github.com/loro-dev/loro/commit/d34abeef929c232b6fce858ee6a4314116ccf202))
- interface update ([a20b4c9](https://github.com/loro-dev/loro/commit/a20b4c9e8b09de31889c1441892039d0c1c359ae))
- it's possible to enter the no sibling state ([284f1e0](https://github.com/loro-dev/loro/commit/284f1e086217a1cfb6fc40f33215620f1e311c01))
- iter bug ([3eae708](https://github.com/loro-dev/loro/commit/3eae708b245ae6551d9ff8089e5bea5178d90126))
- iter bug & lamport bug & set init len for tracker ([9157e75](https://github.com/loro-dev/loro/commit/9157e75ed73f51d2f719f8b4f90a7128609bf5bc))
- iter end ([90596fa](https://github.com/loro-dev/loro/commit/90596fa3e37406336880c56b10b2d932c130acd2))
- lamport issue ([2a0f842](https://github.com/loro-dev/loro/commit/2a0f842fc503467ea1d7d1690f90353e4fadf27d))
- lamport order ([0cbf5e2](https://github.com/loro-dev/loro/commit/0cbf5e25487fee3b147760c6f120eb4716cd19e7))
- lamport remove sort ([033916c](https://github.com/loro-dev/loro/commit/033916c5320d6b1cbcbe21a93b6214f726b0be69))
- list ([c7e5c90](https://github.com/loro-dev/loro/commit/c7e5c907d5c3fd40a939792e51ca41fdf547d8e0))
- list assert err ([c798662](https://github.com/loro-dev/loro/commit/c798662dd1604c7aa87a5a035258a865d000e52d))
- list iter and slice err ([882def3](https://github.com/loro-dev/loro/commit/882def3fc3b5df2c5787ec921f6b9457e0949dfd))
- list op merge logic ([55c274d](https://github.com/loro-dev/loro/commit/55c274d5ace0188c4df26fbdd5e69c89759be888))
- list state err ([fa13d1d](https://github.com/loro-dev/loro/commit/fa13d1d06fe6b3299360073dfd1b4d0adf05a515))
- list use delta as op ([d144906](https://github.com/loro-dev/loro/commit/d14490650283aa9baa9214ecfbe17d54a77ddc11))
- list_op delete merge error ([24773ac](https://github.com/loro-dev/loro/commit/24773ac217de4982a21465f24d70fd288bba516f))
- lookup change ([f0266f0](https://github.com/loro-dev/loro/commit/f0266f015fe8fa7a199cebe07198cb3da8760277))
- loro-crdt type ([aadde5a](https://github.com/loro-dev/loro/commit/aadde5af9ef3a2999f593f7422fd2a5ce87807b3))
- make directly apply faster ([e7b1148](https://github.com/loro-dev/loro/commit/e7b1148c8a74ebf2d69d1a92d5d1a70315bbffc7))
- make encoding enhanced simpler ([9b18ff9](https://github.com/loro-dev/loro/commit/9b18ff9847ba7e7fa7ac6e38b713e7435975ad72))
- make events JsValue ([86057ad](https://github.com/loro-dev/loro/commit/86057adb0596309b5ccbff912f43df6d920efbd8))
- make export less strict ([6c1fef7](https://github.com/loro-dev/loro/commit/6c1fef7c95ffd347562308f49dd462b6c17a434a))
- make it work for text container simple cases ([26a68dc](https://github.com/loro-dev/loro/commit/26a68dc64a975f1e3043d3537cc85c942f84a9b0))
- make LoroCore: Send + Sync ([#61](https://github.com/loro-dev/loro/issues/61)) ([a03c68a](https://github.com/loro-dev/loro/commit/a03c68a993796078125be2d01dd6ca29bb92ba00))
- make LoroValue serialize & deserialize compatible with json and binary ([5ec8752](https://github.com/loro-dev/loro/commit/5ec8752d3df4d8844d7285e8f8544e0d354ecaa4))
- make recursive case work ([f5ae229](https://github.com/loro-dev/loro/commit/f5ae229ca343556da7d5dc630178cea3acfc7a6f))
- make subscription work ([db29178](https://github.com/loro-dev/loro/commit/db2917898241c7531be889fe6562161613c2bab2))
- make text container send&sync ([39f5140](https://github.com/loro-dev/loro/commit/39f514022e89af00018ee0caeb448edecf115487))
- make text event in wasm use utf16 as index and len ([#88](https://github.com/loro-dev/loro/issues/88)) ([3e64116](https://github.com/loro-dev/loro/commit/3e64116621fb0396524d4579f1227075f4f3d1a9))
- map apply order ([8f6059d](https://github.com/loro-dev/loro/commit/8f6059df5ab3d262279cc5c6d4b3063b0be0ab2d))
- map fuzz diff delete LoroValue::Null ([398ddcb](https://github.com/loro-dev/loro/commit/398ddcb25a17b2ba84d27d4d6c36711485546a4c))
- map lamport order ([b6e9983](https://github.com/loro-dev/loro/commit/b6e9983eb7f47b9277f720f02284f592b839d199))
- map pool mapping ([af29f7e](https://github.com/loro-dev/loro/commit/af29f7e20268fe344592adeba859adf4c911c12e))
- map version checkout err ([#101](https://github.com/loro-dev/loro/issues/101)) ([72cc8c6](https://github.com/loro-dev/loro/commit/72cc8c6ed5bf9791dcf622d32dc87f826f0ebd60))
- merge err ([1ce91be](https://github.com/loro-dev/loro/commit/1ce91be046a9ca84ed444359ec539e81c9afc12d))
- merge err ([3ea9770](https://github.com/loro-dev/loro/commit/3ea97708710bebbd22ae2a364187ebd5c573381f))
- mermaid links fix ([4a608cc](https://github.com/loro-dev/loro/commit/4a608cc958c000bed0f62fa4a11c4fb2f426fb38))
- modify after merging ([2d316b4](https://github.com/loro-dev/loro/commit/2d316b4414c88ec154f0ea07077a2d27bcbd1f63))
- nesting notify ([953a461](https://github.com/loro-dev/loro/commit/953a4613c64c78b38481aface0d1453640084fea))
- new encode format ([dad7680](https://github.com/loro-dev/loro/commit/dad768049e4dd5f0c02be5f21a91bf5b42b89ef2))
- no panic when integrate an deleted container ([a7f21e3](https://github.com/loro-dev/loro/commit/a7f21e3f44b70eb74409cd20cfcdf0048b980dbb))
- nodejs patch ([5c0d8b7](https://github.com/loro-dev/loro/commit/5c0d8b74f4e32af2fc6c81d61b614009362c9cb2))
- not leaking closure ([b0d7ad8](https://github.com/loro-dev/loro/commit/b0d7ad88b99c4c21ef74f1bc655d5e4265e6c101))
- notify ([77eac9e](https://github.com/loro-dev/loro/commit/77eac9eb304a0d0b837f34145bb6ef12084a496a))
- notify fuzzy test ([2c11846](https://github.com/loro-dev/loro/commit/2c11846c99573c8109ccb282806dc3651dfc6b3c))
- op content merge ([98c9360](https://github.com/loro-dev/loro/commit/98c9360a8576b6f70c7b7e27f74c8e6b0308864e))
- op converter ([c294c61](https://github.com/loro-dev/loro/commit/c294c61343d1fdf9b5b69e4aa9547284c85aa5ad))
- op counter ([d1d2425](https://github.com/loro-dev/loro/commit/d1d242578f5576ceb21161aee910f1e7e3ce1cdf))
- op iter bug ([3b42c06](https://github.com/loro-dev/loro/commit/3b42c06a01b288b921bbb08ad330518b7034b066))
- opt offset [lamport] ([7c8aa72](https://github.com/loro-dev/loro/commit/7c8aa72969ab266b19a0a6527a386ee84a9dcb9a))
- origin left should points to non-deleted ([7c032b6](https://github.com/loro-dev/loro/commit/7c032b63213e492da6d14ba074463d13a4b25fb3))
- other client pending import (with debug ([9db061e](https://github.com/loro-dev/loro/commit/9db061ed36a44f8fcadc6841e06287021e7dfc66))
- partial iter bug ([037093f](https://github.com/loro-dev/loro/commit/037093f6bd5a933f7e4b23cfa43e5ae7088c3395))
- pass dag prop test ([56be50c](https://github.com/loro-dev/loro/commit/56be50c1b6b9215457f93a0a9532b5a338eb1f34))
- pass delta test ([344bbb1](https://github.com/loro-dev/loro/commit/344bbb1e344bcc443955432c82445e489b1d3f9e))
- path reverse ([870b39e](https://github.com/loro-dev/loro/commit/870b39ec378b789cf0ea8e6f7676182f8d38c396))
- post delete handler ([c8a83fe](https://github.com/loro-dev/loro/commit/c8a83fe67610a72b5ec8dee356dd2595f96d24f0))
- prelim compatible with pool ([4194c79](https://github.com/loro-dev/loro/commit/4194c79fe7b883bf65ad00bb6505b437999c8ff5))
- prelim transaction ([f165e25](https://github.com/loro-dev/loro/commit/f165e2594c7f2783c7b05c4c1c8162bf4ab00a1d))
- range map ([96f29ee](https://github.com/loro-dev/loro/commit/96f29ee0fa2354ccc27cdc1294a3bf35e17e44be))
- redefine and fix find common ancestor ([a9d57bf](https://github.com/loro-dev/loro/commit/a9d57bfc14707750fa1f4180f8f49a5cbd89a73b))
- reduce heap alloc ([9c35aa2](https://github.com/loro-dev/loro/commit/9c35aa266cf4f9f0c67142a9876dc217f377b3eb))
- reduce heap alloc ([43c2860](https://github.com/loro-dev/loro/commit/43c28608c61efd80b4174abca056f2b2587edc74))
- reduce unsafe code ([b80a70b](https://github.com/loro-dev/loro/commit/b80a70bb2d48d1918373c2e49ac7fd50f22f5896))
- refine mermaid diagram style ([bb3eb7b](https://github.com/loro-dev/loro/commit/bb3eb7b7a077a017676c3959e80573883e3d3bf2))
- refine rangemap interface ([1933fe6](https://github.com/loro-dev/loro/commit/1933fe6a56b6380d6ae41de1334595c73c1a6866))
- remove a few unsafe blocks about create cursor ([02ebfbc](https://github.com/loro-dev/loro/commit/02ebfbc0fcb3e011548491318ae7359c6d35fd9f))
- remove auto commit ([04dd105](https://github.com/loro-dev/loro/commit/04dd105d33c26ed6bbd889c768d2cc4f0811706d))
- remove changes error freeze behavior ([58fb7de](https://github.com/loro-dev/loro/commit/58fb7de26c1057694799f1847e2651b08f06b7a0))
- remove checker to container inner ([73598c4](https://github.com/loro-dev/loro/commit/73598c49adf0eb50de0e539a14bdc660d7095a94))
- remove container encoding ([16400dd](https://github.com/loro-dev/loro/commit/16400ddab08cf98fd8575aeb6d7cc3113867d4fb))
- remove counter & lamport from RleUpdates encoding ([ea921e4](https://github.com/loro-dev/loro/commit/ea921e4c8fb40aeebc2bcf5c0d1eeaadd208ccae))
- remove counter & lamport from snapshot ([ccfa3ee](https://github.com/loro-dev/loro/commit/ccfa3ee63d2e061856ed98ad33653fddc631dcef))
- remove delta clone ([3a0b8d9](https://github.com/loro-dev/loro/commit/3a0b8d9d58cfe58eece1a27d7fc6c33f324b29b0))
- remove events when delete container ([8f4b5f1](https://github.com/loro-dev/loro/commit/8f4b5f101a3c89aaaed054d36666ec1e4f667608))
- remove heap ([e3a93be](https://github.com/loro-dev/loro/commit/e3a93be6a255e2ce65636fcec59058e71618a170))
- remove lamport from snapshot ([d87b3b9](https://github.com/loro-dev/loro/commit/d87b3b960df91d26046fa2b5ddbfd1a5a50fe3b2))
- remove needless check ([90fe4cc](https://github.com/loro-dev/loro/commit/90fe4cc69efbc502a28bf61baea096261a292726))
- remove needless notify ([ff9877d](https://github.com/loro-dev/loro/commit/ff9877db4200c5a4347fc281044ec904b29b42a7))
- remove needless notify ([83abf39](https://github.com/loro-dev/loro/commit/83abf398d19c2b5b0a6c8c40ef672504e51a9a4b))
- remove over conservative check ([139d71e](https://github.com/loro-dev/loro/commit/139d71e64a6729ede2abfe616c98d2ae7620feac))
- remove temp, add checker ([4f5f809](https://github.com/loro-dev/loro/commit/4f5f809bb63cf59fa39613f7d351406ec77f0918))
- remove unknown type on content ([780f756](https://github.com/loro-dev/loro/commit/780f756450b2aeb95a051cd67a9421bcffc4107b))
- rename has global index ([7e5c9b0](https://github.com/loro-dev/loro/commit/7e5c9b0b0ffab61d48d2bfff86a118035d2abb30))
- resolve deep value ([1c7ccf2](https://github.com/loro-dev/loro/commit/1c7ccf2b53dc41e516449d0eeaf047a8e2e7f5f6))
- return err when changes should be queued ([ae730d2](https://github.com/loro-dev/loro/commit/ae730d2b8ce5bb179621ab5932be8cae49bd474c))
- return Err when index out of bound ([532eee0](https://github.com/loro-dev/loro/commit/532eee09a427ed34ae8e2bc3ffad6c50030f7c8e))
- return none for deleted container when finding path ([d2123a2](https://github.com/loro-dev/loro/commit/d2123a2099868bd3ca2d5e3b4a185e72b2d46316))
- revert enhanced encoding ([05dc62a](https://github.com/loro-dev/loro/commit/05dc62a31c1636aede14b102e180730fb0d0a7ba))
- richtext event ([#138](https://github.com/loro-dev/loro/issues/138)) ([95e6130](https://github.com/loro-dev/loro/commit/95e6130d930d710f7099a70591a6c405d79668c1))
- rle iter logic ([4784918](https://github.com/loro-dev/loro/commit/478491831df1a27d6b134cb7ba0a208f736cd386))
- rletree creator ([0127690](https://github.com/loro-dev/loro/commit/0127690b1156f36fe92fca0b066c30d01be35f1b))
- rleUpdates lamport calculate ([52dd09d](https://github.com/loro-dev/loro/commit/52dd09db2a7378ff25ba8198805f6b107e876567))
- rm snapshot start counter ([04cec60](https://github.com/loro-dev/loro/commit/04cec6048f3e1ececc08f2ea285428e23f7df9ba))
- seq container vv error ([e385c09](https://github.com/loro-dev/loro/commit/e385c09e11ad05162cc7ed80673108cb6eefef99))
- set small range err ([592199a](https://github.com/loro-dev/loro/commit/592199ab6575cc3aa17a275129ae8cba70f94111))
- settimeout by default in subscription ([94f481e](https://github.com/loro-dev/loro/commit/94f481e65ebad1a5c48a4f36947233fe7e089e03))
- should be readonly when doc is in detached mode ([640828b](https://github.com/loro-dev/loro/commit/640828bf26c0bed1f77d1910a332d9bcb0ce65b7))
- should filter out non-active spans on delete ([a2fcd73](https://github.com/loro-dev/loro/commit/a2fcd73b44ea21dc5888212bb7821da9f1a559e3))
- should keep deleted container id in hierarchy ([8722208](https://github.com/loro-dev/loro/commit/872220851d65e60bbeddda11809f5f20d5f2ad1a))
- should notify err ([c611728](https://github.com/loro-dev/loro/commit/c611728d8891a4fb67764f1265de63b0118e6f3b))
- should use slicerange in text container ([114e129](https://github.com/loro-dev/loro/commit/114e12944de5ad0414522f92f39c61953064d95a))
- simplify create level when apply update ([81b8f2c](https://github.com/loro-dev/loro/commit/81b8f2c591233521e703148766c6ee9b1e29e820))
- simplify get siblings ([6da6cd2](https://github.com/loro-dev/loro/commit/6da6cd29154cb929650dfe84a608afb0a0078998))
- simplify op set ([bc57f01](https://github.com/loro-dev/loro/commit/bc57f01e181aaa80990cde96a78b6dd2dc0514fe))
- slice issue ([ec07825](https://github.com/loro-dev/loro/commit/ec07825c4f426a88f7287077cd1a446470cf8e85))
- snapshot container error due to temp idx ([dc69267](https://github.com/loro-dev/loro/commit/dc6926773c06899a91b93146d69386f56a5d100c))
- snapshot encode err ([#135](https://github.com/loro-dev/loro/issues/135)) ([8235eaa](https://github.com/loro-dev/loro/commit/8235eaaa9b797ef28128a5df697f9c2430e5628a))
- snapshot encoding err ([2a436e0](https://github.com/loro-dev/loro/commit/2a436e07ad96a68ccb1ca654bc2626ac2b451890))
- snapshot load diff ([7ffac80](https://github.com/loro-dev/loro/commit/7ffac80215c5568a06c7dbb6c7c4dc6ac0fadaf8))
- snapshot pending import ([ac80775](https://github.com/loro-dev/loro/commit/ac80775b17987b0a537d0b0d660ddc0d57390634))
- snapshot unknown and change value to i64 ([af6342a](https://github.com/loro-dev/loro/commit/af6342a52d8f35609d8211fcb9f96abda52256f3))
- SnapshotOp map use u32 ([d2b8941](https://github.com/loro-dev/loro/commit/d2b8941f06bc85d0c320c1f62a6c4bb42892dcc8))
- some suggestions about rm lamport ([1743f4a](https://github.com/loro-dev/loro/commit/1743f4af42b6a0761eb922e1acd2c157476bf6ee))
- sort change by ord ([e196e23](https://github.com/loro-dev/loro/commit/e196e23581cbad87611e082aa6dc96c7a290d3ea))
- sort key -lamport ([de12fc8](https://github.com/loro-dev/loro/commit/de12fc8da9498fa55217ac73a72d37def53389bd))
- speed up encode ([6020198](https://github.com/loro-dev/loro/commit/60201989ec2ea2a893c2d39af673489985b9cba1))
- still apply op from deleted container ([fcffc29](https://github.com/loro-dev/loro/commit/fcffc2924f40604ab6111ee6d215bcceea40580a))
- string fuzzy ([1df1f1d](https://github.com/loro-dev/loro/commit/1df1f1d2bffff8f1759760f79eb31ee975f58956))
- styling ([0241567](https://github.com/loro-dev/loro/commit/02415676eac8931b5a7e6c099056b5ba3bee34c4))
- test ([38ccf36](https://github.com/loro-dev/loro/commit/38ccf36b9d2a4a0ccd70a0242e51918fb06effde))
- text container heads update ([93af1c7](https://github.com/loro-dev/loro/commit/93af1c72c5eecfc2f2cd7154f4520703211ab1e7))
- text sync issues ([abec22c](https://github.com/loro-dev/loro/commit/abec22cd223fac89b0d670543f0ba9cbbbb8b58e))
- to json result ([b1738e3](https://github.com/loro-dev/loro/commit/b1738e34a941d043f299d4c1d8fe6b109dfd4f21))
- to_json resolve deep ([3faaf25](https://github.com/loro-dev/loro/commit/3faaf259918bc45432aa3ac1d0360fbd7a4e68ea))
- to_json resolve deep ([11292e3](https://github.com/loro-dev/loro/commit/11292e3337956f91906411f247d9d6580bd53789))
- to_json resolve deep ([8a3f205](https://github.com/loro-dev/loro/commit/8a3f20524e437c54ed8e37ff834783b89e1d663e))
- tracker state fix ([12e2899](https://github.com/loro-dev/loro/commit/12e2899bdd5098d16f4acd7b161789b7cbba04d2))
- transaction ([74a7aa6](https://github.com/loro-dev/loro/commit/74a7aa6c1a0b78e1b34222a4af28ae4d8793c1c8))
- transaction op apply ([1979c23](https://github.com/loro-dev/loro/commit/1979c2312585e5d6ccb78c71403b54c46c9f4c16))
- tree balance issue ([d36c41b](https://github.com/loro-dev/loro/commit/d36c41b7cd6c44fdab33c18550cfb1b674ceef97))
- try merging parent after its children removed ([7191668](https://github.com/loro-dev/loro/commit/7191668a65459c3dc2d58b47e3529e330cef6fa4))
- try to avoid recursive lock in notification ([05f19de](https://github.com/loro-dev/loro/commit/05f19de9ded7d732852a07a37d5800472f42f5ea))
- try to put origin right to an un deleted elem ([0509db4](https://github.com/loro-dev/loro/commit/0509db416b5ffc04f40eb14dfc6c1e1ad47366de))
- two sites sync issue ([69521a9](https://github.com/loro-dev/loro/commit/69521a97efde9c15d2f8af484d7c5bdc1d84d87b))
- type err ([3a0c00f](https://github.com/loro-dev/loro/commit/3a0c00fdecd97b08df5103ed32284df631a97c2e))
- type err ([8a15d2e](https://github.com/loro-dev/loro/commit/8a15d2e86308aed67149727864ab0e6a457f4597))
- type error ([cb26a46](https://github.com/loro-dev/loro/commit/cb26a46b9e0dc6cc8b809ac7dfa2a330a50eb5f6))
- typo on op -> diff ([aa151a4](https://github.com/loro-dev/loro/commit/aa151a48f54c2fca93a0e08419c427b7b8f1e619))
- ues try_lock ([cf1f7dc](https://github.com/loro-dev/loro/commit/cf1f7dc443796598835b27e5543b920a362f24ad))
- unsound (violate borrow stack rules) bugs detected by Miri ([#32](https://github.com/loro-dev/loro/issues/32)) ([f757b86](https://github.com/loro-dev/loro/commit/f757b86f5c1bec4ac7ccdc5b0fa9aa3657906b79))
- unsubscribe ([8545a0a](https://github.com/loro-dev/loro/commit/8545a0aa7536a5e3fb74b2542787d319a8dd9f72))
- update bumpalo fix potential leaks ([737c14e](https://github.com/loro-dev/loro/commit/737c14e99a8d4c44d2072a2307acdc4171e384be))
- update crate path ([fbb2403](https://github.com/loro-dev/loro/commit/fbb2403f8fa56c811b7b27fa2dfe09c445d951e3))
- update deps ([cc3a869](https://github.com/loro-dev/loro/commit/cc3a869ee4dcd38c33b9a40a3659172791189e39))
- update leaf cache when create new elem by del ([e30ba86](https://github.com/loro-dev/loro/commit/e30ba86653f6caa8395c1159999e74e4fd9a906c))
- use &mut instead of BumpBox for Node ([12e2937](https://github.com/loro-dev/loro/commit/12e29374dc2e3829516ff1a536d1397e7c010f06))
- use after free in heap mode when deleting ([f7821f0](https://github.com/loro-dev/loro/commit/f7821f05153ef6f9214b0871a3cec8e7e0d5aacb))
- use better structure for log store access ([4fc987f](https://github.com/loro-dev/loro/commit/4fc987f42a1c940adff0b3e7de9dd224b56d1d70))
- use BTreeMap to iter node content ([7ec25b3](https://github.com/loro-dev/loro/commit/7ec25b396e767b0c975e33a3cd4e5342451cd3eb))
- use container id when converting unresolved to jsvalue ([16e76ea](https://github.com/loro-dev/loro/commit/16e76eaf8e422b0cf88735cada6ebf1ef3d7b794))
- use heap mode in text state ([62891e2](https://github.com/loro-dev/loro/commit/62891e25b3e1973b84ce9213935e72bac249f5aa)), closes [#8](https://github.com/loro-dev/loro/issues/8)
- use LoroValue as json content ([2e1d508](https://github.com/loro-dev/loro/commit/2e1d5080a53995027aac91b2818070d0805de594))
- use LoroValue as json content ([fabef2a](https://github.com/loro-dev/loro/commit/fabef2ad3579e2bf3e40857fdd8309511b4fa0b9))
- use Option as delta meta ([449a772](https://github.com/loro-dev/loro/commit/449a77254d9d183be95ba095257e0a1cb9d02c28))
- use promise.then instead of timeout ([32378a7](https://github.com/loro-dev/loro/commit/32378a71882928f99888021ee05b3a2fa233523c))
- use split_off to take n ([6fd81c8](https://github.com/loro-dev/loro/commit/6fd81c8d20ff170760fa6dd3dd34631763e8cd99))
- use stack instead of heap ([5707258](https://github.com/loro-dev/loro/commit/57072585a1c9f1f6cc4624872a21cbc7061e9ae3))
- use topological sort for causal iter ([ee22d29](https://github.com/loro-dev/loro/commit/ee22d295737c42a1db090e4cd6974e25ac8624ba))
- use Transaction to decode/import ([#92](https://github.com/loro-dev/loro/issues/92)) ([e51d6f8](https://github.com/loro-dev/loro/commit/e51d6f8760bdebc5aff202aa3cb7af3e1e7e8d9a))
- use update encoding by default ([bd3ac43](https://github.com/loro-dev/loro/commit/bd3ac432032b07ec3147caea5bb112619251e8bd))
- use utf16 by default for text in wasm ([4c37235](https://github.com/loro-dev/loro/commit/4c372359e696694516029d9bfe9772d700d1f88d))
- utf16 len fallback to utf8 when unknown ([#93](https://github.com/loro-dev/loro/issues/93)) ([bbcb6f3](https://github.com/loro-dev/loro/commit/bbcb6f39577857f8b6876bd4e8210ca61e677cb1))
- utf16 query err ([#151](https://github.com/loro-dev/loro/issues/151)) ([5a9baeb](https://github.com/loro-dev/loro/commit/5a9baebba0148dfd2113320b4dab9b916c183c21))
- vec slice bug ([cf3e3ee](https://github.com/loro-dev/loro/commit/cf3e3ee3616de9c07c4442e396a5c7da478367d3))
- vec slice is ill defined ([610a651](https://github.com/loro-dev/loro/commit/610a651b5c02b654ad2ebe28bb4524c5a2481a8e))
- warnings ([200a6dd](https://github.com/loro-dev/loro/commit/200a6dd39adcdb0e998bc718b7b977f207448659))
- wasm add client id check ([9734860](https://github.com/loro-dev/loro/commit/973486067ae9aa911e60bec1043e631e7710df53))
- wasm change peerid should be bigint ([48611c5](https://github.com/loro-dev/loro/commit/48611c5f15615924719b46dd96ce09c44e898c20))
- wasm hierarchy notify dead lock ([80640ca](https://github.com/loro-dev/loro/commit/80640ca4e1812dc6cdcf4569575993cfcb06977b))
- wasm interface ([e124bbb](https://github.com/loro-dev/loro/commit/e124bbbec10d6c91a6ccf43dbec40927315b77e8))
- wasm loro class inner mutability ([6a02ce1](https://github.com/loro-dev/loro/commit/6a02ce1568c6674747603daa85d035f56b2ffd36))
- wasm type convert err ([3c7e939](https://github.com/loro-dev/loro/commit/3c7e939020dd834530ce3ed1f02d2e2386a9d88b))
- yata fuzzing now works ([b11fe73](https://github.com/loro-dev/loro/commit/b11fe7394e0ea5094f9bebd43570d56b8abfdc0b))
- yata fuzzing works now ([7f728db](https://github.com/loro-dev/loro/commit/7f728db4952d37a5f62c217bf8cac08266750b6d))
- yata id spans generate bug ([280382c](https://github.com/loro-dev/loro/commit/280382c39cbe034df42b306e1fca38875949f5db))
- yspan slice bug ([8406b18](https://github.com/loro-dev/loro/commit/8406b182ae0717c955dba008a96fdd044f808ecc))
