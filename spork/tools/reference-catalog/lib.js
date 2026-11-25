// Helpers every catalogue script starts with (robot.py puts this file before the script). They
// run inside the sandboxed reference app, in its Rhino engine, so Java classes are at hand.

var IJ = Packages.com.intellij;
var Files = java.nio.file.Files;
var Paths = java.nio.file.Paths;

function str(value) {
  return value === null || value === undefined ? null : String(value);
}

function writeJson(path, value) {
  Files.createDirectories(Paths.get(path).getParent());
  Files.writeString(Paths.get(path), JSON.stringify(value, null, 1));
}

// null once closed (an IDE's Welcome window has none; the reference app's is a project of its own)
function project() {
  return IJ.openapi.project.ProjectManager.getInstance().getOpenProjects()[0] || null;
}

function frame() {
  return IJ.openapi.wm.WindowManager.getInstance().getFrame(project());
}

// Paints `component` (a window's root pane, a popup's content) into a PNG at the screen's
// scale; nothing is read from the screen, so no capture permission is needed.
function shot(component, path) {
  var width = component.getWidth(), height = component.getHeight();
  if (width <= 0 || height <= 0) return false;
  var scale = IJ.ui.scale.JBUIScale.sysScale(component);
  var image = new java.awt.image.BufferedImage(
    Math.ceil(width * scale), Math.ceil(height * scale), java.awt.image.BufferedImage.TYPE_INT_ARGB);
  var g = image.createGraphics();
  g.scale(scale, scale);
  component.paint(g);
  g.dispose();
  Files.createDirectories(Paths.get(path).getParent());
  javax.imageio.ImageIO.write(image, "png", new java.io.File(path));
  return true;
}

// The text a renderer component shows: its own, or that of the first label inside it.
function rendered(component) {
  if (component === null) return null;
  if (component instanceof IJ.ui.SimpleColoredComponent) return str(component.getCharSequence(false));
  if (component instanceof javax.swing.JLabel || component instanceof javax.swing.AbstractButton) return str(component.getText());
  if (component instanceof java.awt.Container) {
    var kids = component.getComponents();
    for (var i = 0; i < kids.length; i++) {
      var text = rendered(kids[i]);
      if (text) return text;
    }
  }
  return null;
}

// A list or combo item as its renderer draws it (an item's own text is often an object id).
var scratchList = new javax.swing.JList();
function itemText(renderer, value, index) {
  try {
    return rendered(renderer.getListCellRendererComponent(scratchList, value, index, false, false)) || str(value);
  } catch (e) {
    return str(value);
  }
}

// What a component shows a user, depth first: its kind, text, state, items, and where it is
// in `root`. Containers without anything of their own are folded into their children.
function dump(component, root) {
  root = root || component;
  if (!component.isShowing()) return null;
  var J = javax.swing;
  var node = {type: str(component.getClass().getSimpleName()) || str(component.getClass().getName())};
  var add = function (key, value) { if (value !== null && value !== undefined && value !== "") node[key] = value; };
  if (component instanceof J.AbstractButton) {
    add("text", str(component.getText()));
    if (component instanceof J.JToggleButton) node.selected = component.isSelected();
  } else if (component instanceof J.JLabel) {
    add("text", str(component.getText()));
  } else if (component instanceof J.text.JTextComponent) {
    add("text", str(component.getText()));
  } else if (component instanceof J.JComboBox) {
    var renderer = component.getRenderer();
    add("selected", itemText(renderer, component.getSelectedItem(), -1));
    var items = [];
    for (var i = 0; i < Math.min(component.getItemCount(), 80); i++) items.push(itemText(renderer, component.getItemAt(i), i));
    node.items = items;
  } else if (component instanceof IJ.ui.TitledSeparator) {
    add("text", str(component.getText()));
  } else if (component instanceof IJ.ui.SimpleColoredComponent) {
    add("text", str(component.getCharSequence(false)));
  } else if (component instanceof J.JTree) {
    var rows = [];
    for (var r = 0; r < Math.min(component.getRowCount(), 300); r++) {
      var path = component.getPathForRow(r);
      var last = path.getLastPathComponent(), text = null;
      var leaf = component.getModel().isLeaf(last);
      try {
        text = rendered(component.getCellRenderer().getTreeCellRendererComponent(component, last, false, component.isExpanded(r), leaf, r, false));
      } catch (e) {}
      rows.push({depth: path.getPathCount() - 1,
        text: text || str(component.convertValueToText(last, false, component.isExpanded(r), leaf, r, false))});
    }
    node.rows = rows;
  } else if (component instanceof J.JList) {
    var model = component.getModel(), list = [], cells = component.getCellRenderer();
    for (var k = 0; k < Math.min(model.getSize(), 300); k++) {
      var value = model.getElementAt(k), shown = null;
      try { shown = rendered(cells.getListCellRendererComponent(component, value, k, false, false)); } catch (e) {}
      list.push(shown || str(value));
    }
    node.items = list;
  } else if (component instanceof J.JTable) {
    var columns = [];
    for (var c = 0; c < component.getColumnCount(); c++) columns.push(str(component.getColumnName(c)));
    var cells = [];
    for (var row = 0; row < Math.min(component.getRowCount(), 100); row++) {
      var line = [];
      for (var col = 0; col < component.getColumnCount(); col++) line.push(str(component.getValueAt(row, col)));
      cells.push(line);
    }
    node.columns = columns;
    node.cells = cells;
  }
  if (component instanceof J.JComponent) add("tooltip", str(component.getToolTipText()));
  if (!component.isEnabled()) node.enabled = false;
  // a component in another window than `root` (a popup's) has no position in it
  try {
    var at = J.SwingUtilities.convertPoint(component.getParent() || component, component.getX(), component.getY(), root);
    node.bounds = [at.x, at.y, component.getWidth(), component.getHeight()];
  } catch (e) {
    node.bounds = [component.getX(), component.getY(), component.getWidth(), component.getHeight()];
  }
  var children = [];
  if (component instanceof java.awt.Container && !(component instanceof J.JTree) && !(component instanceof J.JList)
      && !(component instanceof J.JTable) && !(component instanceof J.JComboBox)) {
    var kids = component.getComponents();
    for (var n = 0; n < kids.length; n++) {
      var child = dump(kids[n], root);
      if (child !== null) children.push(child);
    }
  }
  var own = Object.keys(node).filter(function (key) { return key !== "type" && key !== "bounds" && key !== "enabled"; });
  if (own.length === 0 && children.length === 1) return children[0];
  if (own.length === 0 && children.length === 0) return null;
  if (children.length > 0) node.children = children;
  return node;
}

// The windows showing now, newest last.
function windows() {
  var all = java.awt.Window.getWindows(), shown = [];
  for (var i = 0; i < all.length; i++) if (all[i].isShowing()) shown.push(all[i]);
  return shown;
}

function title(window) {
  return window instanceof java.awt.Dialog || window instanceof java.awt.Frame ? str(window.getTitle()) : null;
}

// The first showing component under `root` that `test` accepts, depth first.
function findBy(root, test) {
  if (root.isShowing() && test(root)) return root;
  if (!(root instanceof java.awt.Container)) return null;
  var kids = root.getComponents();
  for (var i = 0; i < kids.length; i++) {
    var found = findBy(kids[i], test);
    if (found !== null) return found;
  }
  return null;
}

// The first showing component under `root` whose class is named `name` (without its package),
// depth first.
function findByClass(root, name) {
  return findBy(root, function (c) { return String(c.getClass().getName()).split(/[.$]/).pop() === name; });
}

// What a context menu of tool window `id` opens on: its component of class `name`, or its first tree.
function inToolWindow(id, name) {
  var tw = IJ.openapi.wm.ToolWindowManager.getInstance(project()).getToolWindow(id);
  if (tw === null) return null;
  var found = name ? findByClass(tw.getComponent(), name)
    : findBy(tw.getComponent(), function (c) { return c instanceof javax.swing.JTree; });
  // a row selected, as right-clicking one does
  if (found instanceof javax.swing.JTree && found.isSelectionEmpty() && found.getRowCount() > 0)
    found.setSelectionRow(Math.min(1, found.getRowCount() - 1));
  return found;
}

function toolWindow(id, show) {
  var tw = IJ.openapi.wm.ToolWindowManager.getInstance(project()).getToolWindow(id);
  if (tw !== null) { if (show) tw.activate(null); else tw.hide(null); }
}

// Brings the Log editor tab to the front, as the reference app opens it; true when there is one. An IDE has
// the log in the Version Control tool window instead.
function showLog() {
  var fem = IJ.openapi.fileEditor.FileEditorManager.getInstance(project());
  var files = fem.getOpenFiles(), tab = false;
  for (var i = 0; i < files.length; i++)
    if (String(files[i].getClass().getSimpleName()) === "DefaultVcsLogFile") { fem.openFile(files[i], true); tab = true; }
  if (!tab) toolWindow("Version Control", true);
  return tab;
}
