/** Vendor marks. Claude's is a 24x24 path from the icon set the Paseo app ships (02_研究/research/paseo/packages/app/
 *  assets/icons); Codex's is its own app icon (the cloud with ">_" in ChatGPT.app's icon-codex-*.png), outline traced
 *  into 12x12 with its blue-to-lavender colours; CodeBuddy's is the 12x12 mark from its CLI package (dist/web-ui/
 *  pwa-icon.svg); Kimi's is redrawn from its CLI's web logo (kimi_cli/web/static/logo.png: a white K and a blue dot on
 *  black). The marks belong to Anthropic, OpenAI, Tencent and Moonshot AI. */
import type { Vendor } from "./limits";

/** Agents with a mark: the two with usage limits, plus CodeBuddy and Kimi. */
export type Mark = Vendor | "codebuddy" | "kimi";

/** Bar colours: Claude's orange, the blue of the Codex cloud. */
export const VENDOR_COLORS: Record<Mark, string> = { claude: "#d97757", codex: "#5560f6", codebuddy: "currentColor", kimi: "#000" };
const VIEWBOX: Record<Mark, string> = { claude: "0 0 24 24", codex: "0 0 12 12", codebuddy: "0 0 12 12", kimi: "0 0 12 12" };

const PATHS: Record<Mark, string> = {
  kimi: "M2.6 0h6.8A2.6 2.6 0 0 1 12 2.6v6.8A2.6 2.6 0 0 1 9.4 12H2.6A2.6 2.6 0 0 1 0 9.4V2.6A2.6 2.6 0 0 1 2.6 0z",
  codebuddy: "M0.0001 5.9182L0.0001 6.0001L0.0001 6.0821C0 8.8444 0 10.2426 0.8788 11.1214C1.0848 11.3274 1.3193 11.4851 1.5932 11.6058C1.5625 11.2148 1.6512 10.8775 1.855 10.6444C1.9344 10.5539 1.9369 10.5501 1.9196 10.398C1.8909 10.149 1.878 9.7805 1.891 9.5427L1.9014 9.3205L1.5679 8.7305C1.0513 7.8115 0.7233 7.0398 0.5967 6.4501C0.5299 6.1268 0.534 5.9834 0.6161 5.8773C0.6661 5.8132 0.8298 5.7469 1.0273 5.7104C1.5245 5.6231 2.6087 5.7021 3.815 5.9151L3.9403 5.9367L4.2156 5.6931C4.6728 5.2883 4.9764 5.0612 5.5363 4.7122C6.1198 4.3471 6.7783 4.0468 7.5198 3.809L7.7578 3.7327L7.8885 3.3893C8.3568 2.1531 8.8364 1.2418 9.1784 0.9386C9.296 0.8331 9.3031 0.829 9.3894 0.8238C9.5292 0.8136 9.6572 0.8807 9.8752 1.0791C10.3844 1.5418 11.0934 2.4931 11.5342 3.3054L11.7045 3.6207L11.9451 3.7403C11.955 3.7453 11.9652 3.7505 11.9756 3.756C11.9193 2.3548 11.7335 1.4908 11.1214 0.8788C10.2426 0 8.8444 0 6.0821 0.0001L6.0001 0.0001L5.9182 0.0001C3.1558 0 1.7576 0 0.8788 0.8788C0 1.7576 0 3.1558 0.0001 5.9182ZM12.0001 5.5869C11.3059 4.9017 10.2729 4.6581 9.3399 4.9737C9.0955 5.0564 8.8304 5.2094 8.3004 5.5154L5.2558 7.2732C4.7258 7.5792 4.4608 7.7322 4.2669 7.9026C3.481 8.5933 3.1852 9.6974 3.5204 10.6886C3.6031 10.9331 3.7561 11.1981 4.0621 11.7281C4.1171 11.8234 4.1672 11.9101 4.2133 11.9894C4.7159 12.0002 5.2807 12.0001 5.9182 12.0001L6.0001 12.0001L6.0821 12.0001C8.8444 12.0002 10.2426 12.0003 11.1214 11.1214C12.0003 10.2426 12.0002 8.8444 12.0001 6.0821L12.0001 6.0001L12.0001 5.9182C12.0001 5.8055 12.0001 5.6951 12.0001 5.5869ZM9.3152 7.2263Q9.5309 7.1018 9.7715 7.1662Q10.0122 7.2307 10.1367 7.4464L10.7843 8.5681Q10.9089 8.7838 10.8444 9.0245Q10.7799 9.2651 10.5642 9.3896Q10.4606 9.4494 10.342 9.465Q10.2234 9.4806 10.1079 9.4497Q9.9923 9.4187 9.8974 9.3459Q9.8025 9.2731 9.7427 9.1695L9.0951 8.0478Q9.0353 7.9442 9.0197 7.8256Q9.0041 7.707 9.035 7.5915Q9.066 7.4759 9.1388 7.381Q9.2116 7.2861 9.3152 7.2263ZM6.5262 9.0398Q6.4107 9.0088 6.2921 9.0244Q6.1735 9.0401 6.0699 9.0999Q5.9663 9.1597 5.8935 9.2546Q5.8207 9.3495 5.7897 9.465Q5.7588 9.5806 5.7744 9.6992Q5.79 9.8177 5.8498 9.9213L6.4974 11.043Q6.6219 11.2588 6.8625 11.3232Q7.1031 11.3877 7.3189 11.2632Q7.4225 11.2033 7.4953 11.1084Q7.5681 11.0135 7.5991 10.898Q7.63 10.7825 7.6144 10.6639Q7.5988 10.5453 7.539 10.4417L6.8914 9.32Q6.8316 9.2164 6.7367 9.1436Q6.6418 9.0708 6.5262 9.0398Z",
  claude: "M4.709 15.955l4.72-2.647.08-.23-.08-.128H9.2l-.79-.048-2.698-.073-2.339-.097-2.266-.122-.571-.121L0 11.784l.055-.352.48-.321.686.06 1.52.103 2.278.158 1.652.097 2.449.255h.389l.055-.157-.134-.098-.103-.097-2.358-1.596-2.552-1.688-1.336-.972-.724-.491-.364-.462-.158-1.008.656-.722.881.06.225.061.893.686 1.908 1.476 2.491 1.833.365.304.145-.103.019-.073-.164-.274-1.355-2.446-1.446-2.49-.644-1.032-.17-.619a2.97 2.97 0 01-.104-.729L6.283.134 6.696 0l.996.134.42.364.62 1.414 1.002 2.229 1.555 3.03.456.898.243.832.091.255h.158V9.01l.128-1.706.237-2.095.23-2.695.08-.76.376-.91.747-.492.584.28.48.685-.067.444-.286 1.851-.559 2.903-.364 1.942h.212l.243-.242.985-1.306 1.652-2.064.73-.82.85-.904.547-.431h1.033l.76 1.129-.34 1.166-1.064 1.347-.881 1.142-1.264 1.7-.79 1.36.073.11.188-.02 2.856-.606 1.543-.28 1.841-.315.833.388.091.395-.328.807-1.969.486-2.309.462-3.439.813-.042.03.049.061 1.549.146.662.036h1.622l3.02.225.79.522.474.638-.079.485-1.215.62-1.64-.389-3.829-.91-1.312-.329h-.182v.11l1.093 1.068 2.006 1.81 2.509 2.33.127.578-.322.455-.34-.049-2.205-1.657-.851-.747-1.926-1.62h-.128v.17l.444.649 2.345 3.521.122 1.08-.17.353-.608.213-.668-.122-1.374-1.925-1.415-2.167-1.143-1.943-.14.08-.674 7.254-.316.37-.729.28-.607-.461-.322-.747.322-1.476.389-1.924.315-1.53.286-1.9.17-.632-.012-.042-.14.018-1.434 1.967-2.18 2.945-1.726 1.845-.414.164-.717-.37.067-.662.401-.589 2.388-3.036 1.44-1.882.93-1.086-.006-.158h-.055L4.132 18.56l-1.13.146-.487-.456.061-.746.231-.243 1.908-1.312-.006.006z",
  codex: "M11.75 6.0L11.75 6.5L11.75 7.01L11.6 7.5L11.46 7.99L11.23 8.44L10.85 8.8L10.38 9.07L9.94 9.3L9.47 9.47L9.33 9.97L9.02 10.31L8.72 10.71L8.31 10.96L7.9 11.21L7.4 11.21L6.96 11.46L6.48 11.46L6.0 11.2L5.54 11.2L5.13 10.96L4.74 10.7L4.29 10.71L3.81 10.7L3.28 10.71L2.88 10.45L2.48 10.2L2.04 9.96L1.59 9.7L1.27 9.31L1.11 8.82L0.96 8.35L0.97 7.83L0.95 7.35L0.77 6.92L0.52 6.48L0.21 6.0L0.21 5.49L0.21 4.98L0.34 4.48L0.52 4.0L0.94 3.64L1.11 3.18L1.56 2.89L2.18 2.79L2.48 2.48L2.63 1.98L2.94 1.63L3.32 1.37L3.72 1.11L4.13 0.87L4.63 0.87L5.05 0.61L5.53 0.61L6.0 0.86L6.45 0.86L6.86 1.12L7.24 1.38L7.69 1.37L8.15 1.38L8.68 1.37L9.07 1.62L9.48 1.85L9.92 2.08L10.31 2.38L10.69 2.71L10.85 3.2L10.98 3.68L10.99 4.18L10.99 4.66L11.15 5.09L11.46 5.52Z",
};

export function vendorIcon(vendor: Mark, className = "limit-icon"): SVGSVGElement {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", VIEWBOX[vendor]);
  svg.setAttribute("class", className);
  svg.setAttribute("aria-hidden", "true");
  const path = document.createElementNS(ns, "path");
  path.setAttribute("d", PATHS[vendor]);
  path.setAttribute("fill", VENDOR_COLORS[vendor]);
  if (vendor === "claude") path.setAttribute("fill-rule", "evenodd");
  svg.append(path);
  if (vendor === "codex") codexFace(svg, path);
  if (vendor === "kimi") kimiFace(svg);
  return svg;
}

/** Kimi's white K with the blue dot at its top right, on the black square. */
function kimiFace(svg: SVGSVGElement): void {
  const ns = "http://www.w3.org/2000/svg";
  const k = document.createElementNS(ns, "path");
  k.setAttribute("d", "M3.3 3.2h1.35v2.5L7 3.2h1.75L6.1 6l2.85 2.8H7.1L5.15 6.85l-.5.52V8.8H3.3z");
  k.setAttribute("fill", "#fff");
  const dot = document.createElementNS(ns, "circle");
  for (const [a, v] of Object.entries({ cx: "9.25", cy: "2.85", r: "0.8", fill: "#1d8cff" })) dot.setAttribute(a, v);
  svg.append(k, dot);
}

/** The Codex cloud: lavender at the top to blue at the bottom, with a white ">_" on it. */
function codexFace(svg: SVGSVGElement, cloud: SVGPathElement): void {
  const ns = "http://www.w3.org/2000/svg";
  const make = (tag: string, attrs: Record<string, string>) => {
    const e = document.createElementNS(ns, tag);
    for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
    return e;
  };
  const id = "codex-cloud-fill"; // every copy defines the same gradient, so sharing one id is harmless
  const grad = make("linearGradient", { id, x1: "0", y1: "0", x2: "0", y2: "1" });
  grad.append(make("stop", { offset: "0", "stop-color": "#b9a5f9" }), make("stop", { offset: "1", "stop-color": "#3e41f5" }));
  const defs = make("defs", {});
  defs.append(grad);
  svg.prepend(defs);
  cloud.setAttribute("fill", `url(#${id})`);
  const ink = { fill: "none", stroke: "#fff", "stroke-width": "0.95", "stroke-linecap": "round", "stroke-linejoin": "round" };
  svg.append(make("path", { d: "M3.6 4.8L5 6L3.6 7.2", ...ink }), make("path", { d: "M6.3 7.3H8.4", ...ink }));
}
