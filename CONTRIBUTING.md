## Copyright and reverse engineering rules

To avoid any legal risk, this project takes a very strict stance towards reverse engineering. ALL reverse engineering must be conducted black-box, without inspecting any code (source or binary) distributed by Live2D Inc. In addition, we also disallow contributions by people potentially exposed to code from projects with similar reverse-engineering goals, as there is no way to verify that those projects were themselves developed with an equivalently strict policy.

If you have done any of the following at any point, you may not contribute to this project in any form (neither code nor issue reports and discussion):

* Disassembled, decompiled, or otherwise directly inspected any binary software distributed by Live2D Inc., or any application incorporating such components (e.g. VTube Studio)
    * Exception: Incidental exposure due to debugging unrelated issues (e.g. you saw a VTube Studio stack trace or context disasm due to a crash report or while developing Wine/Proton, not involving the Live2D components)
* Obtained any non-public code or software by Live2D Inc., including any "leaked" code or software, or code or software transferred as part of a non-public contract or agreement (this includes binary and source forms), even if you did not analyze/read it
* Read any portion of the code of any other project involving Live2D reverse engineering (including both MOC3 and CMO3 files)
* Pirated any software by Live2D Inc or otherwise engaged in gross copyright or license violation

If you have done any of the following at any point, you may open bug reports against the project, but we ask that you refrain from making direct code contributions or participating in reverse engineering discussions:

* Read any source code (including header files, source code, shaders, etc.) or other information distributed by Live2D as part of a publicly available SDK under a license that is not [OSI Approved](https://opensource.org/licenses). Note that the code that Live2D distributes on their GitHub repositories is, in general, NOT distributed under an OSI approved open source license.
* Entered into a licensing agreement with Live2D Inc to use their technology in a project (when using their public SDK, not any nonpublic components)

The following are permitted:

* Reading documentation on Live2D's website that is not subject to a license agreement (accessible publicly with no click-through or clearly declared license/TOS)
    * Although not banned, please avoid looking up Live2D SDK documentation if possible. We are not interested in cloning the SDK or its API as part of the core Ayagami project. (Editor documentation is OK)

### Usage of Live2D Cubism Editor and other Live2D software

There is no restriction on former usage of the Live2D Cubism Editor or other *publicly available* software by Live2D Inc (i.e. people who are no longer an active licensee/user). 

Using Cubism Editor or other software distributed *directly* by Live2D Inc as an *active* component in black-box reverse engineering is not allowed (do not intentionally create test models with the editor for the sole purpose of RE; do not load models created externally into such software for the sole purpose of RE). To minimize risk, we strictly use software not distributed directly by Live2D Inc. as a target for black-box analysis (i.e. VTube Studio).

Active users of the Live2D Cubism Editor are not prohibited from contributing to this project, but are advised to exercise caution in doing so, as this could conceivably be a reason for retaliation by Live2D Inc (e.g. revoking their Editor license). It is acceptable to analyze models created for other purposes as part of reverse engineering (i.e. loading a finished model you previously rigged in the Editor into this project, then fixing parsing or rendering issues by observation), but do NOT make changes to the model to further the research. In these cases, we recommend opening an issue instead of making a direct code contribution.

## AI models and agents

For copyright and legal reasons, this project disallows contributions by AI agents or any other machine-learning framework that has been trained or may have been trained on large volumes of information, including but not limited to information scraped from the internet.

Please do not use AI agents to contribute to this project, neither directly or indirectly. Examples of forbidden conduct include using an AI agent to do any of the following:

* Author any code related to this project
* Analyze this repository's code
* Review code intended for contribution
* Research any file formats or structures relevant to this project
* Inquire about Live2D technology internals
* Do any of the above in relation to any other project that interacts with Live2D technology

**If you have already used an AI agent or chatbot to interact with any project related to MOC3 files or research MOC3 files in any way, then you are forbidden from ever contributing to this project. This applies even if you cease to use AI in the future.**

The reason for this strict policy is that **AI usage is fundamentally incompatible with black-box, clean-room reverse engineering**. Clean-room reverse engineering requires that no single person or entity both read or reverse engineer the original software and contribute to the final reverse-engineered implementation. Black-box goes further, and forbids analyzing the original software at all. **This includes software that Live2D, Inc. publishes on GitHub under a source-available but proprietary license, as well as any other projects interacting with MOC3 files which do not have an equivalent black-box policy.** Since AI agents are trained on content scraped from the internet including GitHub repositories, **any output by an AI agent related in scope to MOC3 files or Live2D technology would violate the black-box principle, and compromise the legal position of this project**.

This does not apply to AI usage unrelated to Live2D technology, such as:

* Usage for other projects completely unrelated to Live2D or MOC3 files
* Asking general questions about programming, languages, non-Live2D APIs, etc.
    * The question must NOT include any names, identifiers, or snippets referencing MOC3 or any related concepts, and must NOT include any code directly pasted from the project
    * You must NOT copy and paste the answer directly as part of a contribution (don't ask for ready-made code)
* Usage entirely unrelated to programming and VTuber/2D model technology
* Using AI strictly for machine translation or accessibility purposes

While we discourage usage of AI agents or chatbots in any capacity out of an abudance of caution, the above actions do not disqualify you from contributing to this project.

### Autocomplete

Small-scale, *locally-executed* code auto-complete machine learning models *may* be allowable on a case-by-case basis, if they are deemed to be compact enough to represent a negligible risk of encoding any specific information likely to cause a legal risk to the project. Please contact us by opening an issue if you use an editor with such a feature before using it to contribute to this project, and provide details of the exact model used and how we may evaluate it.

## Security reports & vulnerability research

Due to the black-box requirement explained above, active project code contributors may not use AI to analyze the codebase to search for vulnerabilities or other security issues, as AI output could taint them as contributors.

Anyone may report security vulnerabilities regardless of how they were found or whether they meet the above conditions, as long as they limit their contribution to ONLY making the relevant security bug report with the information relevant to the vulnerability. In particular, if you used AI or otherwise did not meet the conditions in the previous sections, please don't submit a complete fix/patch, and leave that to project maintainers or other contributors. Limit the report to the specifics of the vulnerability needed to understand and fix it (cross-referencing existing code in the project is fine).
