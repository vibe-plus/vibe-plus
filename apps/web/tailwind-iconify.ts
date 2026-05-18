import { addDynamicIconSelectors } from "@iconify/tailwind";
import { lobeIconsCollection, vibePlusIconsCollection } from "./uno-icons.ts";

export default addDynamicIconSelectors({
  prefix: "i",
  iconSets: {
    lobe: lobeIconsCollection(),
    vp: vibePlusIconsCollection(),
  },
  customise(content) {
    return content.replace(/<svg /, '<svg style="display:inline-block;vertical-align:middle" ');
  },
});
