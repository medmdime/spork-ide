// edt
// Everything the reference app registers, as it presents it: actions with their text, description, icon
// and macOS shortcut; groups with their children in order (menus, toolbars, popups); tool
// windows; the Settings tree; and the plugin each comes from (com.intellij: the platform).
// Written to OUT/inventory.json.

var am = IJ.openapi.actionSystem.ActionManager.getInstance();
var keymap = IJ.openapi.keymap.KeymapManager.getInstance().getActiveKeymap();
var KeymapUtil = IJ.openapi.keymap.KeymapUtil;

// the plugin registering each action; actions made at run time (a tool window's Activate…) by
// their class
var pluginOf = {}, plugins = [];
var loaded = IJ.ide.plugins.PluginManagerCore.getLoadedPlugins();
for (var p = 0; p < loaded.size(); p++) {
  var descriptor = loaded.get(p), pluginId = str(descriptor.getPluginId().getIdString());
  plugins.push({id: pluginId, name: str(descriptor.getName()), version: str(descriptor.getVersion()), bundled: descriptor.isBundled()});
  var own = am.getPluginActions(descriptor.getPluginId());
  for (var o = 0; o < own.length; o++) pluginOf[str(own[o])] = pluginId;
}
function pluginOfClass(cls) {
  var descriptor = IJ.ide.plugins.PluginManager.getPluginByClass(cls);
  return descriptor === null ? "com.intellij" : str(descriptor.getPluginId().getIdString());
}

function presentation(action) {
  var p = action.getTemplatePresentation();
  return {text: str(p.getText()), description: str(p.getDescription()), icon: str(p.getIcon())};
}

var actions = [];
var ids = am.getActionIdList("");
for (var it = ids.iterator(); it.hasNext();) {
  var id = it.next();
  var action = am.getActionOrStub(id);
  if (action === null) continue;
  var entry = presentation(action);
  entry.id = str(id);
  entry.class = str(action.getClass().getName());
  entry.plugin = pluginOf[entry.id] || pluginOfClass(action.getClass());
  var shortcuts = keymap.getShortcuts(id);
  if (shortcuts.length > 0) {
    entry.shortcuts = [];
    for (var s = 0; s < shortcuts.length; s++) entry.shortcuts.push(str(KeymapUtil.getShortcutText(shortcuts[s])));
  }
  if (action instanceof IJ.openapi.actionSystem.ActionGroup) {
    entry.group = true;
    entry.popup = action.isPopup();
    if (action instanceof IJ.openapi.actionSystem.DefaultActionGroup) {
      var children = action.getChildActionsOrStubs(), list = [];
      for (var c = 0; c < children.length; c++) {
        var child = children[c];
        if (child instanceof IJ.openapi.actionSystem.Separator) {
          list.push({separator: str(child.getText()) || ""});
        } else {
          var childId = am.getId(child);
          list.push(childId !== null ? {id: str(childId)} : {inline: presentation(child), class: str(child.getClass().getName())});
        }
      }
      entry.children = list;
    } else {
      // its children are computed when shown
      entry.dynamic = true;
    }
  }
  actions.push(entry);
}

var project = project();
var manager = IJ.openapi.wm.ToolWindowManager.getInstance(project);
var twPlugins = {};
var eps = IJ.openapi.wm.ToolWindowEP.EP_NAME.getExtensionList();
for (var e = 0; e < eps.size(); e++)
  twPlugins[str(eps.get(e).id)] = str(eps.get(e).getPluginDescriptor().getPluginId().getIdString());
var toolWindows = [];
var twIds = manager.getToolWindowIds();
for (var t = 0; t < twIds.length; t++) {
  var tw = manager.getToolWindow(twIds[t]);
  toolWindows.push({id: str(twIds[t]), title: str(tw.getStripeTitle()), anchor: str(tw.getAnchor()),
    available: tw.isAvailable(), visible: tw.isVisible(), icon: str(tw.getIcon()),
    plugin: twPlugins[str(twIds[t])] || pluginOfClass(tw.getComponent().getClass())});
}

function configurables(list) {
  var out = [];
  for (var i = 0; i < list.length; i++) {
    var conf = list[i];
    var node = {name: str(conf.getDisplayName()), class: str(conf.getClass().getName())};
    if (conf instanceof IJ.openapi.options.SearchableConfigurable) node.id = str(conf.getId());
    node.plugin = conf instanceof IJ.openapi.options.ex.ConfigurableWrapper
      ? str(conf.getExtensionPoint().getPluginDescriptor().getPluginId().getIdString()) : pluginOfClass(conf.getClass());
    if (conf instanceof IJ.openapi.options.Configurable.Composite) {
      var kids = conf.getConfigurables();
      if (kids.length > 0) node.children = configurables(kids);
    }
    out.push(node);
  }
  return out;
}
var group = IJ.openapi.options.ex.ConfigurableExtensionPointUtil.getConfigurableGroup(project, true);
var settings = configurables(group.getConfigurables());

writeJson("OUT/inventory.json", {
  product: str(IJ.openapi.application.ApplicationNamesInfo.getInstance().getFullProductName()),
  build: str(IJ.openapi.application.ApplicationInfo.getInstance().getBuild().asString()),
  version: str(IJ.openapi.application.ApplicationInfo.getInstance().getFullVersion()),
  keymap: str(keymap.getPresentableName()),
  actions: actions, toolWindows: toolWindows, settings: settings, plugins: plugins,
});
