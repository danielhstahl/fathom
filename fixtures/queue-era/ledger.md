# Fathom review ledger

<!-- format: fathom-review/1 -->
<!--
     One block per human decision, appended oldest first. This file is the durable record:
     the index in .fathom/index.sqlite3 is a rebuildable mirror of it, and deleting the store
     loses no review.

     Do not edit, reorder or re-wrap a block that has been committed. The `digest:` line at the
     end of each block hashes the lines above it, so an edit is detectable and `fathom review
     verify` will name it. To correct a decision, append a new block saying so.

     `members:` are the commits the decision covered, `content:` is the digest of the file
     contents it covered, and `fathom review status` re-checks both against the current history.
-->

## sg-8bbb8edb887f5147 | accepted | 2023-11-17T05:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-71520944056837be8190
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T05:46:40Z
- fingerprint: 71520944056837be81902a53fc6be2662dedb06388d3cc3ce6cecc8332454a06
- fallback: 7d363d1c61157411e2da99a28a0ec6fc706793cd864ac59b5601204c47847ed3
- shape: b82969f9d560da59df6a8c8327e3a85ae87e5fb0bb7098b5ac6cc2dbbbe50507
- content: f2287d2a26af706a8410d2309dc4018ad8cbd9fea49c3ad50b57c6341f7f54a2
- members: dfa76c79
- paths: .gitignore@38867c575160, README.md@921e62b71405, src/mod0.py@6f798d45536c
- scale: 1 commit(s), 3 file(s), +12 -0
- risk: 26 (spine-1)
- digest: sha256:3dee0e235c3b79e6794557c121f1e2037b0611d4b9b6dd1dfa66e92025e8eb96

## sg-a200e3f913dfb03f | accepted | 2023-11-17T06:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-476b8bed2623bc36d363
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T06:46:40Z
- fingerprint: 476b8bed2623bc36d36326df1dc7fce1cd941fa73b3de9e7bf3e29233da22312
- fallback: 103c717816e9dddba81a7167eb98a881a4ec42721cb26fc445fe024cf76e352e
- shape: 794b65c00aa4f98674b8d6a79d7d2dcaa346f1c0187868b7619d8752bee098e5
- content: e6a875d0e779ecbfceae882c86a3e021d2c88cb3eb797e762353f55f06182071
- members: 727fde8f
- paths: src/mod1.py@2287110d3507, tests/test_mod1.py@48ea1b3c5ae4
- scale: 1 commit(s), 2 file(s), +14 -0
- risk: 24 (spine-1)
- digest: sha256:f715f78af33fdc69cdd9901390db2459a694b401999ddd83a7602631560e8ce1

## sg-8570731787201763 | accepted | 2023-11-17T07:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-a4bd1672d00f3be1b1fe
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T07:46:40Z
- fingerprint: a4bd1672d00f3be1b1feaf6fe49934c07f2927765a6759f4290f3849c200f4a1
- fallback: 0ca2ba265f14aa224ab7e62806b228d054cc1a4373f2e18c0b4033f3d3e737c5
- shape: 41c732fcc81e428796edbcb8ee04834a307da6e1c12114be519c43f7c14cc37c
- content: b318dcf8f276c3eae4659c70e0c1e6d7e8b9c2a430c6dbaf8bf0d6332d43f559
- members: 6f38544f
- paths: docs/notes2.md@58c91984b5e3, src/mod2.py@49af83c80113
- scale: 1 commit(s), 2 file(s), +22 -0
- risk: 25 (spine-1)
- note: read the diff; the retry ceiling is what the pager budget needs
- digest: sha256:e0d6a4bd096c088e828dc5bc6374ecf21bf27b60c87732f3b1c198076433bf00

## sg-837deb298f6d72be | accepted | 2023-11-17T08:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-6daa66ff5d57e969a3b0
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T08:46:40Z
- fingerprint: 6daa66ff5d57e969a3b045df56222314b16cea80a4a055119f74c3e07b311c10
- fallback: e378befeef835601ebb12c403705677ae2f9f2eac09c28526a48612c9911a0a5
- shape: f4d297aa67ec91a8080e4b6e837b85db9cdfc0a5d3a778fa1b5b7c6c1863259f
- content: 66120c7d645c142e59a9c3ca38442f514de09b80cde92308849c48ecccd1d099
- members: 0412cc08
- paths: src/mod3.py@61b1013add2c
- scale: 1 commit(s), 1 file(s), +24 -0
- risk: 15 (spine-1)
- digest: sha256:513e3bc7473789a3de6f49c4e874f4b27e67d1f7cba2e4f5d7fcef0b775dc3f1

## sg-b4830eb3b3b8a5fd | accepted | 2023-11-17T09:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-7f56e0929e7e2f3c9c62
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T09:46:40Z
- fingerprint: 7f56e0929e7e2f3c9c627b06e9a5234ac06a89740df98928c5d369b8fb8996a1
- fallback: 61b5804ca3fbd2f8a5e80f6f3a06bd650961a7a08e0c400af4c03fed07a656b0
- shape: d4dc122da5c97046a8f7c8f4e3dcf9776927ec8f941e0ab5e59105e8ce8f043a
- content: 294693c64928e029047a529ffa22642f9ae87d836cad0b3d2ad816f14a54e71b
- members: 54cc9fbb
- paths: src/mod3.py@533d1af92b90, src/mod4.py@1f01231a7955, tests/test_mod4.py@65917b8365c8
- scale: 1 commit(s), 3 file(s), +41 -23
- risk: 27 (spine-1)
- digest: sha256:e04eaf4a79acc212d40ddd7a936d263bd82525157019f08f2e982267a33facc3

## sg-2d08d8a25fa1f88f | accepted | 2023-11-17T10:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-9aa0075d4ba4fb56332d
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T10:46:40Z
- fingerprint: 9aa0075d4ba4fb56332dbd50e7b5157bcb6ca53e295e064d3ddc65b63143106a
- fallback: d61f0a6c8c44b48d5993f47516d71033643110c5aece9cf3da690959f0cdd721
- shape: 684e76058e925c607d0b5f2d9838f31e0ac34a644221d9dd102001d3a3ceae36
- content: 348bdd70ee60b3d6aef7cacc6e8ec18bb7002e10afdc45eeb63f1a9453573790
- members: 675006e5
- paths: docs/notes5.md@39a7c8d2f599, src/mod5.py@343c1cab5423
- scale: 1 commit(s), 2 file(s), +46 -0
- risk: 26 (spine-1)
- note: third pass over this one; it is what the ticket asked for
- digest: sha256:a58e2c02d36b41e7a2f3ab83449eecf50ed67f0552a86824f1cace23bacb9a6f

## sg-74554ed4881c69c5 | flagged | 2023-11-17T11:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-aa5aa6c5032920ffea74
- decision: flagged
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T11:46:40Z
- fingerprint: aa5aa6c5032920ffea741b28a764c10f2b303e53be2f1745c1a243f48b1a2814
- fallback: a1ccc93766c3470d2d1832408b0b61b5ed3b4dbcd75184c501722e2756340bd2
- shape: f45557d83a8c4074c0c54f3acf2f7176158f676142052c545787275121e543b8
- content: 45717ab8e048a5af7a7533f35c82d675df638bd3993816d6a61a1fc837449585
- members: d6bc5755
- paths: src/mod6.py@5b45fd0f1333
- scale: 1 commit(s), 1 file(s), +5 -0
- risk: 21 (spine-1)
- note: the migration has no down step and the runbook is not written
- digest: sha256:b4d9a1c95720dfb575764609008ef827793374a9690cace2f105f235c61cfa47

## sg-0e29836eea0a41c7 | flagged | 2023-11-17T12:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-b4a71bd60b6bfa3de4b2
- decision: flagged
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T12:46:40Z
- fingerprint: b4a71bd60b6bfa3de4b2894c983a2328dfd9890ed79fd7fed402310091ea065a
- fallback: 568842aabb1de820c98ef9596ad34602afc5ca30f44f0e592b8e544478bf2727
- shape: cdcbf594fd173b5e1fb82234f5b052798faeb9397781beca94f762aef6ccbfe9
- content: cfb0a4c829d06b6c05d10437097f03b327557c816c446628ef8666cda79462b4
- members: b3347efc
- paths: src/mod7.py@5eb77fce80b7, tests/test_mod7.py@c588b97c77ca
- scale: 1 commit(s), 2 file(s), +18 -0
- risk: 17 (spine-1)
- note: two agents touched this file in one session and neither said why
- digest: sha256:b9ebe04fd49f35610aea5b4975270becc5b02c1128ca508e59f3ad2fc4395ddb

## sg-f2e03af6c8b7619d | flagged | 2023-11-17T13:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-ad4c4a666197deca88f6
- decision: flagged
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T13:46:40Z
- fingerprint: ad4c4a666197deca88f6e97a9a6f12a8769dba14c3a6e59c9ae7bb022e6b25ae
- fallback: a6b451d4bc227bc48cb828a531be8f9ddbb7aea7095322821ea22e195e59558c
- shape: 9e6f0c0d095f8948a7c7d2b393d2e9e7170a645aaa2fca869a2937aba5de3690
- content: 820a152230e32acad5c4320d78468a46058643f9fb28e4dc3db1ade2e8825305
- members: b575ae88
- paths: docs/notes8.md@da03dacfe17b, src/mod7.py@533d1af92b90, src/mod8.py@11e9e8e85592
- scale: 1 commit(s), 3 file(s), +23 -11
- risk: 26 (spine-1)
- note: the test that covers this is deleted in the same turn
- digest: sha256:d4dcebd5d03e00ca1a182e5c7a0deb0a1fa2d0bb2d39a476fba6055c07363221

## sg-79295b7a6e70c2be | flagged | 2023-11-17T14:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-41aedbc357ac00de4c88
- decision: flagged
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T14:46:40Z
- fingerprint: 41aedbc357ac00de4c88f61157c2a96a12581ccd6ae17e4ba56c5c2b551c05b9
- fallback: 3402f14dfbc7d723357bd44796d647c48ccd69d1cfdefa0ef27ebec0258d93e4
- shape: 133d54c658a861b8e2f7c19f6489e86ffcff67c7fc84941e5ed0149dd1a40888
- content: 4c2f67b192fae75f6eb3f953c32c353aa9d58557e682431a32bf96c9c1d69c2f
- members: ac83889d
- paths: src/mod9.py@a1c34519106e
- scale: 1 commit(s), 1 file(s), +26 -0
- risk: 23 (spine-1)
- note: 1.1k lines of generated output hide the one line that changed
- digest: sha256:8cbcc1db88f2851d10a7f1852a9668e26633657c6c61e75ffb4a1a022d1dd123

## sg-96f7be383991a825 | flagged | 2023-11-17T15:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-88f04b02322be448fe15
- decision: flagged
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T15:46:40Z
- fingerprint: 88f04b02322be448fe15be0cb68b8ed00e49925cfb439bddfa13f8fdff47a394
- fallback: 681157a460b6cd21d2131acc8a685d5e8f046b7b261943b2add90b7109675a34
- shape: 68f882d383502d6873f8bf16aad85bae462cac9594b7eb78729d557b1d7ce11a
- content: 5030baff5a7f63ffbe325e83f52ccea356baae5215b0a3ca969759f71f04a3ec
- members: 897f4b0b
- paths: src/mod10.py@f8278d4f34e0, tests/test_mod10.py@7b7e3d30d5c1
- scale: 1 commit(s), 2 file(s), +35 -0
- risk: 26 (spine-1)
- note: blast says 3 and the graph stopped early; I do not believe the 3
- digest: sha256:e41fbc735522febea7190eb5fbadd83986708a293f1d1151fd8f4978ce75c1a2

## sg-5950d2dea386cb18 | accepted | 2023-11-17T16:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-b21a56f3d7d3061b6c64
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T16:46:40Z
- fingerprint: b21a56f3d7d3061b6c648f11661fc37f3f114559517485dd8d2ad9735e81329a
- fallback: ea61a7928d726c720892b4ccb68f2fe1f0b7963b81afc0f81576d562ef7ab215
- shape: f83e9a25c63711fc4c394199838dd65f8336c6331ddbdf3c386fe9b019745b4a
- content: 0ead6ea7b2a1004ea90ee692d2ba6e7dac562f085206bb50f7075eaa98fd0bfe
- members: 42f902d2
- paths: docs/notes11.md@e92d07a41536, src/mod11.py@7d9398d0829b
- scale: 1 commit(s), 2 file(s), +47 -0
- risk: 18 (spine-1)
- digest: sha256:21e17d54fd23496e0de25a1745c8c769b0cccf4e954a523199b903bf258f8aee

## sg-2a1d0a4d42d030ad | accepted | 2023-11-17T17:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-c7ae72157eaee406a175
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T17:46:40Z
- fingerprint: c7ae72157eaee406a17595ab8651e3c71a5d34f03dc0f13a9fe53d4b90376956
- fallback: 937d751a6e7a491bf14f740ec5ec71ed8ef7188d9354a76146bf2c4a1d3780a1
- shape: 3beebe59ece8dc71fd06e10eb3781ffce9a84ecafc1023e94805e9572a914fde
- content: e9b37dbb2a5ae5eac4fa6ea259963be85a439532e92e42c7135c26bb63d0354d
- members: 37078ccd
- paths: src/mod11.py@533d1af92b90, src/mod12.py@458d181f1bc3
- scale: 1 commit(s), 2 file(s), +7 -39
- risk: 24 (spine-1)
- digest: sha256:731dc1672e36e0d32b1718ceb76ada024b5f7fa81e88947548300c8e799606db

## sg-482fc4977ed8193a | accepted | 2023-11-17T18:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-68af57fbe5ebb34b018c
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T18:46:40Z
- fingerprint: 68af57fbe5ebb34b018c25fcca7ecd3c222154fdc70065a588132dc95ae7650d
- fallback: 21c3a9a3bfd85e1e28934ec248648f3510ce7ee68227b0412853d122ab24ad23
- shape: 0c9f9a33cc638c52b7e6b64d08f2e7c7105e309c9d175a46abfe0a2666c3dd70
- content: c72cd3e53d8d35a054063d3a367c0e9cfd3209524511a27605d1ae0d7f73c17f
- members: ac1a23f7
- paths: src/mod13.py@72a3cd1622e6, tests/test_mod13.py@b1251871dbdb
- scale: 1 commit(s), 2 file(s), +22 -0
- risk: 25 (spine-1)
- digest: sha256:b0eb1b3b1d3403b4b8a80e628c4b76a649de251bb4059128b1d16d3589775675

## sg-52cef48055918eef | accepted | 2023-11-17T19:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-b98bd14030751065eb79
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T19:46:40Z
- fingerprint: b98bd14030751065eb79e4f59b9ef15c0e17faf241e26273378c05d89fb016b2
- fallback: de3485e870a7ee6e01c315a3499b3a4e4249b217f4ad4647665170c980feba5a
- shape: f337f723f8540400cce7ad97322d6e933d9a1fa75ae18eb2e6de6ad2bb5758df
- content: a8bac9c3d3923a0193278576405ab4889367e7abc4cad895d702d8df5f54a6f3
- members: 2051cd95
- paths: docs/notes14.md@367cb26527d3, src/mod14.py@fbcf89cd7081
- scale: 1 commit(s), 2 file(s), +24 -0
- risk: 25 (spine-1)
- digest: sha256:b21b72144481f208edfa22ac118239ab732c8e4dc7bf6b5e9c8b2dab1625f3e6

## sg-4240979df16c574a | accepted | 2023-11-17T20:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-4fc02c20ade00f16ba17
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T20:46:40Z
- fingerprint: 4fc02c20ade00f16ba170cc05bd9071a6ba1922dfe864afa3aa9fb86a7a0c60e
- fallback: c1bd87af97e18a1129c144359bdc36b07889b7dcd9f261c1248525087588f6ec
- shape: a2a549990259cecba8f51236c775c3b41e739ab6331b5df20c65c9d235c333e0
- content: 806f03a2faedc6539189de8b76c42c99c283c30d668b7b79f5c6da99798ca6c5
- members: 190a7817
- paths: src/mod15.py@d2c0a9d6f24e
- scale: 1 commit(s), 1 file(s), +28 -0
- risk: 15 (spine-1)
- digest: sha256:8af84b937093188e80dcff0a9fefcee8c47e0c24c0cb52a3bfcc72d7feff6c94

## sg-8e2c3ec21fc78d6a | dismissed | 2023-11-17T21:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-55459b21e70af911f63b
- decision: dismissed
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T21:46:40Z
- fingerprint: 55459b21e70af911f63b30198f52648d06fc10b272af24d87656d7fcb53f3734
- fallback: f44dfe9169e1c1791149646c3ede94296a8174563b81ffec116c20ff22962657
- shape: f20389d42b4f7597e3dbb2761b826594b7f22293d3a178106406ca9e657ece2c
- content: d68d205a8e9e94bcc690d10f4ac0467478cd4b873df09e4442bfab9bb60b5e0a
- members: c17be9db
- paths: src/mod15.py@533d1af92b90, src/mod16.py@4a00ce6ea8a4, tests/test_mod16.py@48ea1b3c5ae4
- scale: 1 commit(s), 3 file(s), +39 -27
- risk: 27 (spine-1)
- note: not-my-area
- digest: sha256:8203a1553b46b576af80f5541c0afd46cb787917eb83ac5f99583cfb4c844b30

## sg-9f652bdef9c19038 | dismissed | 2023-11-17T22:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-df4912b19b86f72f52c9
- decision: dismissed
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T22:46:40Z
- fingerprint: df4912b19b86f72f52c9452f8e6a510aa7ce4551a6f918f16c3fd825f0aee1f9
- fallback: e074812c130eb9e007a666c199e7cb78059b1a7ba4309bd9980b929ef4936353
- shape: 4211ede74588b523538cd6e609771f1718b0ce1abc233475a3c00445e082a507
- content: da4ee53934c57bedf22ca659529dc3e0aa9f853b3367c2bc30a4ddbd32d9672c
- members: 6bbd4efb
- paths: docs/notes17.md@6cc9e34d7e75, src/mod17.py@98b4f2d53959
- scale: 1 commit(s), 2 file(s), +48 -0
- risk: 26 (spine-1)
- note: not-my-area
- digest: sha256:560d47b71e69994473a47bb49b29f51ca151a8710ad01e5cfded5121e3c3577f

## sg-34f09d183512b585 | dismissed | 2023-11-17T23:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-62408475bd57c74ec781
- decision: dismissed
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-17T23:46:40Z
- fingerprint: 62408475bd57c74ec78133234e59fa1cc9ff0dcd8d90f8a2539e247729a0d312
- fallback: a1473ee546bfb49f6bdca587a1e5a1b0e295f4c2178efb5f1f8e2ae7d5850093
- shape: b5f5c3e59bcaade1d0033f440cc41861c5292b3f91e6261a1a4a35699ee0d45f
- content: 61ab63104a042a19d71f1374f1799f89daf13c485cb4ad6fa7471fd3bd221b26
- members: 51b1323b
- paths: src/mod18.py@71df2323fe3b
- scale: 1 commit(s), 1 file(s), +9 -0
- risk: 22 (spine-1)
- note: already-known
- digest: sha256:a9299f4cccb16191d320c4e4eaf436955076aa98a3eefd1aeccccf475f78baa2

## sg-68ae33b6a467e8ca | dismissed | 2023-11-18T00:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-8099edbd19fb36abd749
- decision: dismissed
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T00:46:40Z
- fingerprint: 8099edbd19fb36abd7494d8e0bb524e7328459db53c7f6a68fb9c6876a667b20
- fallback: 2bc689274396e15bd4c2624366aa52bcb027dfd6bc9248817a43b3854d6de457
- shape: d6da212be3dd53a5067fca306919830d51aee74fe7f24fb38ab2872435c536f5
- content: 52a213212b13b93c839d23b1955c77a8481ac10fdd9ee6dc0fbfe41f463fbcbe
- members: 17ff6331
- paths: src/mod19.py@5c4a8517ed7b, tests/test_mod19.py@65917b8365c8
- scale: 1 commit(s), 2 file(s), +26 -0
- risk: 17 (spine-1)
- note: already-known
- digest: sha256:f3a9078b3840bc3ae946215f02a98cd63e75c0cf8a301ad9d894bde7fcf74e55

## sg-cb309e11b00937c5 | dismissed | 2023-11-18T01:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-90acb7fa900a512b9a6d
- decision: dismissed
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T01:46:40Z
- fingerprint: 90acb7fa900a512b9a6dccf8ff1e447e76ec61f304fb69ba522299613c09d5db
- fallback: cc179e2f2f6bfae37b2640c807bb1f1a510244975d9104ae390d7daea5432c36
- shape: 4ec1b6aeb08b2bf07592dca61d1a3428a8e0e4a6e68c30f7f965d443d098e5fa
- content: 1de3280e67dfca6044607e30f4e13e546c9e2020c4a9cc0672eb36d9179f9557
- members: dbaae247
- paths: docs/notes20.md@942fa5ae9379, src/mod19.py@533d1af92b90, src/mod20.py@fee570883549
- scale: 1 commit(s), 3 file(s), +32 -15
- risk: 26 (spine-1)
- note: the widened retry is what I asked for
- digest: sha256:6b0bb9d15d18d06ef7e9ab072c9146a66d0e1ad420ec1d1f650bc47b3dbd6e88

## sg-9d31aa21f0a65b10 | dismissed | 2023-11-18T02:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-c957b0cf1a46e0b726e6
- decision: dismissed
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T02:46:40Z
- fingerprint: c957b0cf1a46e0b726e6c56db9ba01ef60ac4ae4444b5d71c9a07a02f32dc399
- fallback: d046c3b53954aa43a9abe004f8e40354ea9dc19c3dc5509d54b8e4dcf924d78d
- shape: 0052631f9da91e52fac5e30e69c3caad3a84a90ac08639defda688638a580638
- content: 3adb61797df59463b3de537274c0087af579d99beee13b2344f1f13f87344f1d
- members: 0fb07ea6
- paths: src/mod21.py@2ba4d80f2929
- scale: 1 commit(s), 1 file(s), +30 -0
- risk: 24 (spine-1)
- note: the cycle runs through a test fixture the analyser reads as production
- digest: sha256:bc210cbd5990293ce3a964f5105510f38bc08560a18b8224f8130a7aa25597f2

## sg-b329e14fd98e76e1 | dismissed | 2023-11-18T03:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-9e5b594739d4411e1cda
- decision: dismissed
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T03:46:40Z
- fingerprint: 9e5b594739d4411e1cda8864c0b144baa8ecd9a65cea6da77aea4caa68e83a77
- fallback: 87eedc0517aaaf3d376f639f5dcd287548a55eb32013b1b89be02ad9b139987f
- shape: d63a3d905f8aac09e461b0575f0ce6a81ace4f0e68cecf4f27648afdc50d10d1
- content: 370c096cc73cbe18627914fcecfa0c148adb77e7c102066c01f91d305bf89bee
- members: f334ef68
- paths: src/mod22.py@408a2f2eb313, tests/test_mod22.py@c588b97c77ca
- scale: 1 commit(s), 2 file(s), +43 -0
- risk: 26 (spine-1)
- note: low-stakes
- digest: sha256:4170c77020d4f7ea01724dead45a2cda9b71a63283e4eec0fba12dbf1b100d34

## sg-beab54aaa800ddf1 | dismissed | 2023-11-18T04:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-b9ccf07ca3afd9a04e16
- decision: dismissed
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T04:46:40Z
- fingerprint: b9ccf07ca3afd9a04e168747f10bb8350c63efc8a66287a48c6d86b09bffc5d7
- fallback: c23847efc585f39711b3fcd6519dc65d88462243d86922a4a7681740c033f83b
- shape: 043fbfb70fe4b4a9a103e729e9423301e8134a33e586227c6f84e08f12c46e4c
- content: ffd2257239c4ef4d7b934df17205b7ff7718702d92a9ce9df204be53c3a0b0e0
- members: 98c63803
- paths: docs/notes23.md@58c91984b5e3, src/mod23.py@cb5658297875
- scale: 1 commit(s), 2 file(s), +9 -0
- risk: 16 (spine-1)
- note: low-stakes
- digest: sha256:31e0f6c10e22587ca4df682f4f1757a15ca3f119f3fc1c3a64abe553430d32ec

## sg-553785ded9f8dd3b | dismissed | 2023-11-18T05:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-81a9c63c0f4cf23779cc
- decision: dismissed
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T05:46:40Z
- fingerprint: 81a9c63c0f4cf23779cc8e131a5963238a87badd09630d7583628c015db1a6f7
- fallback: 70d6bce00a6f0b37b96e15187c72f017bba241c251360697a1fcb8c9f914b8b8
- shape: f19df56c43425d38638acc17df489f58c96fed76346351b4f6a1e38fe4fb08d8
- content: 1cf5003812ebb9fc9b9971f307d7f81a62f3f027703de6d4a06cd422a8367afc
- members: 2cbab6fc
- paths: src/mod23.py@533d1af92b90, src/mod24.py@0c107d988cda
- scale: 1 commit(s), 2 file(s), +11 -3
- risk: 22 (spine-1)
- note: superseded
- digest: sha256:202fb69a2baa507f69e9a7715aa92ed5b0711bd7e2295f7adddae132ceff1ba5

## sg-f14e2dfa56bdad23 | dismissed | 2023-11-18T06:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-d2fa5de65fa24d7da011
- decision: dismissed
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T06:46:40Z
- fingerprint: d2fa5de65fa24d7da011641eabb7dce5e4d392b364264c9bb24eb27f8b72d56c
- fallback: 563076c55581573183a0210e6cc5e45a8b4396b6fb7da578d1fc33c39b2267b4
- shape: b769a5ea8b1c9b2e247e5e480893ebfa6df22bcb25d1e548549a4ab2e1bdd421
- content: a4034458a3e999c87acfa9ea590d5de307b5baa1b5b1ad6d3b0e644a0d1c0e00
- members: 3136dd88
- paths: src/mod25.py@7ef056910b7c, tests/test_mod25.py@7b7e3d30d5c1
- scale: 1 commit(s), 2 file(s), +20 -0
- risk: 25 (spine-1)
- note: the whole flag comes out before the merge
- digest: sha256:7b4a5a4b4828bc918a8e072fda7012bae9009d0e30b09b501c1e60adff9ced42

## sg-b92605cd4f68e552 | dismissed | 2023-11-18T07:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-a5b2e1a337d5e9ac66a1
- decision: dismissed
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T07:46:40Z
- fingerprint: a5b2e1a337d5e9ac66a1722881ea0bb306d9ae0eac6fb1913bf76157b86a971d
- fallback: 468d010ee5740f4c297fa1a01dd0df2dc29ba3befc828ff92554eacfb7ac6f6a
- shape: 931ba3cd2fac640df824779cb886b865a60d932a394569c1181d93c4e99dd432
- content: 47e7b121c77f42e7650226a81e94b40cb8704e9908c4dda3eec86565052720cd
- members: 85d0b1cf
- paths: docs/notes26.md@39a7c8d2f599, src/mod26.py@7ced34d793a6
- scale: 1 commit(s), 2 file(s), +33 -0
- risk: 26 (spine-1)
- note: the vendor stream for this session is half written; I will re-read when it is whole
- digest: sha256:0163bd6986ad117f5f082fa6b9c3b99d810876b180fc9a976d7c4c18e68690ea

## sg-b521b650849c99e2 | accepted | 2023-11-18T08:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-b9ada7eede03202e1526
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T08:46:40Z
- fingerprint: b9ada7eede03202e152645d88ca1badfa2e38a2c79854bf446ea324ed4b181dd
- fallback: 71e14916fbd426b8cb90fcf566113cc3f14bdee7950078e4991c2da1ed2c9a96
- shape: 85d7887282ed15700482821e2fb7973589bb796b02517ccd3bf9f9263a672ee9
- content: aff51be1ca48b04ec7f1cb5e206eb40ef8ad77d133d2044d713c29564bab2bc0
- members: 1fb800f0
- paths: src/mod27.py@58c9d4178241
- scale: 1 commit(s), 1 file(s), +32 -0
- risk: 16 (spine-1)
- digest: sha256:7bf6c5adc4a7c6262b2ad45d7d56784d170ab20cc2a126c7eb22e46a838d0028

## sg-e557e5eb25d04434 | accepted | 2023-11-18T09:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-bbfcea399df47d42cf8f
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T09:46:40Z
- fingerprint: bbfcea399df47d42cf8f737c91d5ba22fe4a2882de0b7471768d3290e88b96a3
- fallback: 0d750e80c4d55bb66831437c571c191e3f7c18d1431a2fbca99921f8a524a457
- shape: 61cf23c91a1de7bf40f52f6eb6416e5566cbf4bbebd568b346139a112f313b2a
- content: e86dd98055d31c7e4919d2afe0ead57f6563b07713900661eb017668d2bfb729
- members: 751defbb
- paths: src/mod27.py@533d1af92b90, src/mod28.py@ca023ed5a12b, tests/test_mod28.py@b1251871dbdb
- scale: 1 commit(s), 3 file(s), +47 -31
- risk: 27 (spine-1)
- digest: sha256:41562c6356dffa04aa7ecc49ab2196851525c872638c99bc848c44bb1de9b04e

## sg-ae7a8d5c92e2e7c8 | accepted | 2023-11-18T10:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-9b6912a01945b72c5641
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T10:46:40Z
- fingerprint: 9b6912a01945b72c564152e9d83458c7895144bb82fac8392563aa6ed6525eb1
- fallback: f574dcb673bc48c64cc3f35c17770ebfd6097663de2a6c602853706b24113519
- shape: 171ea4df588dca5301c5f89b1b673c88b44bbd32d5dc3b480fe695aa0d631d53
- content: f96bc4bec23520085e4e43e84a106c1a2917534245d972bb36d31da5afc65837
- members: 927ab938
- paths: docs/notes29.md@da03dacfe17b, src/mod29.py@82ccc9d3b88e
- scale: 1 commit(s), 2 file(s), +10 -0
- risk: 24 (spine-1)
- digest: sha256:7fbac75383abb7cf231c59ea4421e6f1b3aa4d0cd0c972c62d46f4f63413d29a

## sg-8437b16212cef713 | accepted | 2023-11-18T11:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-3ce72de68c5b400881d1
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T11:46:40Z
- fingerprint: 3ce72de68c5b400881d169afd9c00567d9741872f4cf8f54dde4b67d68780d05
- fallback: 7a567a8cde737a72d4e542f6eae97aee095cbcf3f8384a2285b38057e0e75834
- shape: 34e0edf2b20d9380da8e18fa9e2ac25d00d9accece915b00818037c1488429cc
- content: 24471d73a89d359a8823dce3e9ba27642f35844604d83f13898f039930d1a79e
- members: f8fd296d
- paths: src/mod30.py@0dabcb118d33
- scale: 1 commit(s), 1 file(s), +13 -0
- risk: 22 (spine-1)
- digest: sha256:91e850a754752432be0ec025405b1f9ecb564f19b83e319997115a280f045849

## sg-68f3d586cb7cd9c5 | accepted | 2023-11-18T12:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-6418bca63c9ce91b55a0
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T12:46:40Z
- fingerprint: 6418bca63c9ce91b55a0df8208d56502bf15c00e8a6046b34c1cdc2d81e140fa
- fallback: 6a9e6b2d3b755b22a272eb842d3b269207e8b96d026f959705e1260f12cc20f5
- shape: 73afd015052e5bc77f80d6a6b28b1135d62a0ea683d84a99bd4a053d8d7cb171
- content: 8d1f7f5d97bf8d157c1c7ecad9d81f2ea76586816ed48101b35f35a7ec8a4d08
- members: 6cbfab97
- paths: src/mod31.py@c8acf4d1756a, tests/test_mod31.py@48ea1b3c5ae4
- scale: 1 commit(s), 2 file(s), +24 -0
- risk: 17 (spine-1)
- digest: sha256:d3bfb64312380c7e7ff278c12a7f07a6b2f6f9dc9c7213a5dc23e090e5bdb9c8

## sg-03272f5eba20ed51 | snoozed | 2023-11-18T13:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-f667b8fe768bb4e55d0f
- decision: snoozed
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T13:46:40Z
- fingerprint: f667b8fe768bb4e55d0f41c7c8437409eb432b5dd9046cc51dceeaffb4dd6405
- fallback: fe95114d507bb64cfd7f5eb52e1edda7b88b3ea95561261528dba7eeb1150c32
- shape: 4034a91e6d77aacd1ac55120661cbb65e87adf4927a9d420243fcf18abbe870d
- content: da6ad292e5f3477d0099a2030b6da1d869bf79ae7f7b615a95b348ba17e0c47f
- members: 74a672eb
- paths: docs/notes32.md@e92d07a41536, src/mod31.py@533d1af92b90, src/mod32.py@66cf31b7f6cc
- scale: 1 commit(s), 3 file(s), +34 -19
- risk: 26 (spine-1)
- digest: sha256:e06e1de4fe05c02ed58a9764b952585a2fefec390f3cd3a9f6d6b2e0c57554bf

## sg-f13e1a8f7901597c | snoozed | 2023-11-18T14:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-d402a0b38a89df9feec4
- decision: snoozed
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T14:46:40Z
- fingerprint: d402a0b38a89df9feec41cef36f77d4b584341f364a9b88bca0651caf3863fb4
- fallback: fefd894d4666eaf84aa1f34f5af5d93b323d5af5ffe9b74447a2f8657f998944
- shape: ea5b65dfc82a64d49527cd32f49957e04eabb484cba2f4d70124a4348710b4a8
- content: eedb585f9b485652499149dbbb938a2d5cb96864f9dc5cfae8652ed2f4562437
- members: e6101117
- paths: src/mod33.py@8f4104c30521
- scale: 1 commit(s), 1 file(s), +34 -0
- risk: 24 (spine-1)
- digest: sha256:0cf4ab9c8be601f3978e303329099bd44f9ba31705581e73925a09a5ee761952

## sg-dd56a42cc448c629 | snoozed | 2023-11-18T15:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-d6d87c53f23fe3d750af
- decision: snoozed
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T15:46:40Z
- fingerprint: d6d87c53f23fe3d750af2bc14d8729c074d2c68ec38d1d474c858e87609c9b97
- fallback: 60944e20b611b588287a0dfc2a482bc677790179311d4d9cf37fc543cad3ca98
- shape: e3ef44e8e844699096babf48f485c96d9620a57cdf5f5f05801e26431b2b2c8b
- content: 0e6b4a53906d97a5095f463ec6f460fdda0b3cd65642d0ececf7cd808bc98426
- members: bd978393
- paths: src/mod34.py@259de4c4e158, tests/test_mod34.py@65917b8365c8
- scale: 1 commit(s), 2 file(s), +51 -0
- risk: 26 (spine-1)
- digest: sha256:887dab33e306ce63d68a14d890572ad7349e291f3e8f477ab2f3a1f6e204c6fa

## sg-57171e04df9b6203 | snoozed | 2023-11-18T16:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-094ea1435045f9681bc1
- decision: snoozed
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T16:46:40Z
- fingerprint: 094ea1435045f9681bc1eeae132677b0a23607f8e6c16fcf7af1f1862eb16a9a
- fallback: 827fe031dbb707f308bc5111495647d91f077ef5285e8a892c872db9c7c972fa
- shape: c88486f38178e8ee6f2ccb6b1989b70ff035d19f0fed38ff2e3892173e003217
- content: 6d568f47e242be0b8f47acf6aedb2fcf62a03507f5e332db7cad9142edf5ab4a
- members: 70bb041d
- paths: docs/notes35.md@367cb26527d3, src/mod35.py@3fdf216acb4b
- scale: 1 commit(s), 2 file(s), +11 -0
- risk: 16 (spine-1)
- digest: sha256:2138c381dd1ecec52b62821783dc7a7d5131486d7d988f6c4574fd9346557f23

## sg-00ab25021c026666 | accepted | 2023-11-18T17:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-5a4528d14148bdce2f1b
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T17:46:40Z
- fingerprint: 5a4528d14148bdce2f1bfbe5bf0f85eaadf49c02b72a09db3cbd10d5b8349ce9
- fallback: ceb09fdcac2a47008b61bb150066a93021427fff575f6c1f835a1020290cc0fd
- shape: 558a6e2f961d7ef9c8c94172f4b7239e777891a729d9c12335698c6cd89a66df
- content: 460612056f98b04de602bf7025d1bd687806b7096b24d46fd1eac4912c6d930a
- members: 6686b65f
- paths: src/mod35.py@533d1af92b90, src/mod36.py@995870f4c8c4
- scale: 1 commit(s), 2 file(s), +15 -7
- risk: 23 (spine-1)
- digest: sha256:70f14f43f4d0b7cab8a6ac60827b57ae61f24aeeb987a2cda486fdafecd56c83

## sg-7bfc3a059917122f | accepted | 2023-11-18T18:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-0a86d86c76733a901321
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T18:46:40Z
- fingerprint: 0a86d86c76733a901321be5bcd326012a1cc4a25219c7bec17a15a8c24784566
- fallback: 6fb957b84424d0f63f15ef3de92749ab77374ed59239e5b0fd61a358bb99fd92
- shape: 57b0c08d4ffb45728fe9ec40c7eef374b5e2bc1f915a52c8f868da119113ebb8
- content: 4740454d3485789622e8fbfe39ae46e825c96a4487655aab21dd875ae24f763c
- members: 155de556
- paths: src/mod37.py@2e4a1b9e5b6d, tests/test_mod37.py@c588b97c77ca
- scale: 1 commit(s), 2 file(s), +28 -0
- risk: 25 (spine-1)
- digest: sha256:9ebe8c69fcaec555ec63aefd928a92d5db3989dd10567a9f4f251cd9997a0bae

## sg-937c750a4e637a97 | accepted | 2023-11-18T19:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-2977dfff338f4f44b714
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T19:46:40Z
- fingerprint: 2977dfff338f4f44b7148213b9c432f64bc93df7f51b0ccfab914cc829db4d4d
- fallback: b256041b547b552098ee05c9e55ba7cf9a85f6aaf7500900cb57a4edd2cfb939
- shape: c9daabedd9ead67cde9abad42eab5583ec55ad8f44f92c407c147184941fd80c
- content: 2c27132859140c5dcf07de2f2ffec4058132d15a11972845613a016ed1568fe6
- members: 7a050de7
- paths: docs/notes38.md@6cc9e34d7e75, src/mod38.py@2354500b20ca
- scale: 1 commit(s), 2 file(s), +35 -0
- risk: 26 (spine-1)
- digest: sha256:d0c4f6f9e980d554bba2967019a96c9feb47ea99436e86392b3e8033d2795732

## sg-c818fc62ac0b9c7a | accepted | 2023-11-18T20:46:40Z | Dana Stahl <dana@example.com>

- episode: ep-781636df74527490f124
- decision: accepted
- actor: Dana Stahl <dana@example.com>
- actor-kind: human
- signed: 2023-11-18T20:46:40Z
- fingerprint: 781636df74527490f124fcb37de40e60ecca439eae1b9062d1a24338eb97256c
- fallback: 80a567c386250d51bc01bfddc52c89db84ff2cf4b092a2b1c21600999da46486
- shape: 7cd66d9b41e7d3048964cab743dcf5c5c5aef544d473ac3fb8e92038a94e6ab7
- content: de3c536f9889d13d0ce513af0a0351b78dd48b48371d875920eeb46b8de6b91a
- members: 9aaa45fa
- paths: src/mod39.py@01ef28c2e436
- scale: 1 commit(s), 1 file(s), +36 -0
- risk: 16 (spine-1)
- digest: sha256:7e2a97dbe4354f5372f36abdca281fd35acef78281cbdfcd48427bbe242448a2
