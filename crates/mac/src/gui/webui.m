// Settings window (NSWindow + NSVisualEffectView + WKWebView).
// ui/app.html renders, webui.rs reads/writes values; this file only owns the window and the bridge.
// Call everything on the main thread (AppKit / WebKit requirement).
#import <Cocoa/Cocoa.h>
#import <WebKit/WebKit.h>

// Implemented in webui.rs
extern void knit_webui_on_message(const char *json);
extern void knit_webui_on_close(void);

// Transparent strip over the title-bar area so the window stays draggable
// (a full-size WKWebView would otherwise swallow the drag).
@interface KnitDragStrip : NSView
@end
@implementation KnitDragStrip
- (void)mouseDown:(NSEvent *)event {
    if (event.clickCount == 2) {
        [self.window performZoom:nil];
        return;
    }
    [self.window performWindowDragWithEvent:event];
}
- (BOOL)acceptsFirstMouse:(NSEvent *)event {
    (void)event;
    return YES;
}
@end

@interface KnitWebHandler : NSObject <WKScriptMessageHandler, WKNavigationDelegate, NSWindowDelegate>
@end

@implementation KnitWebHandler
- (void)userContentController:(WKUserContentController *)controller
      didReceiveScriptMessage:(WKScriptMessage *)message {
    (void)controller;
    if (![message.body isKindOfClass:[NSString class]]) {
        return;
    }
    knit_webui_on_message([(NSString *)message.body UTF8String]);
}

// Never navigate away inside the window; https links open in the default browser.
- (void)webView:(WKWebView *)webView
    decidePolicyForNavigationAction:(WKNavigationAction *)action
                    decisionHandler:(void (^)(WKNavigationActionPolicy))decisionHandler {
    (void)webView;
    NSURL *url = action.request.URL;
    if (action.navigationType == WKNavigationTypeLinkActivated && [url.scheme isEqualToString:@"https"]) {
        [[NSWorkspace sharedWorkspace] openURL:url];
        decisionHandler(WKNavigationActionPolicyCancel);
        return;
    }
    if (action.navigationType == WKNavigationTypeOther && ([url.scheme isEqualToString:@"about"] || url == nil)) {
        decisionHandler(WKNavigationActionPolicyAllow);
        return;
    }
    decisionHandler(action.targetFrame.isMainFrame && action.navigationType == WKNavigationTypeOther
                        ? WKNavigationActionPolicyAllow
                        : WKNavigationActionPolicyCancel);
}

- (void)windowWillClose:(NSNotification *)notification {
    (void)notification;
    knit_webui_on_close();
}
@end

static NSWindow *gWindow = nil;
static WKWebView *gWeb = nil;
static KnitWebHandler *gHandler = nil;

void knit_webui_show(const char *html, const char *title, double width, double height) {
    if (gWindow == nil) {
        gHandler = [KnitWebHandler new];
        NSRect frame = NSMakeRect(0, 0, width, height);
        NSWindowStyleMask style = NSWindowStyleMaskTitled | NSWindowStyleMaskClosable |
                                  NSWindowStyleMaskMiniaturizable | NSWindowStyleMaskResizable |
                                  NSWindowStyleMaskFullSizeContentView;
        gWindow = [[NSWindow alloc] initWithContentRect:frame
                                              styleMask:style
                                                backing:NSBackingStoreBuffered
                                                  defer:NO];
        gWindow.releasedWhenClosed = NO;
        gWindow.titlebarAppearsTransparent = YES;
        gWindow.titleVisibility = NSWindowTitleHidden;
        gWindow.title = [NSString stringWithUTF8String:title];
        gWindow.minSize = NSMakeSize(760, 520);
        gWindow.delegate = gHandler;
        gWindow.tabbingMode = NSWindowTabbingModeDisallowed;
        [gWindow setFrameAutosaveName:@"KnitSettingsWeb"];

        // Sidebar material (the HTML sidebar is transparent and lets this show through)
        NSVisualEffectView *material = [[NSVisualEffectView alloc] initWithFrame:frame];
        material.material = NSVisualEffectMaterialSidebar;
        material.blendingMode = NSVisualEffectBlendingModeBehindWindow;
        material.state = NSVisualEffectStateFollowsWindowActiveState;
        material.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;

        WKWebViewConfiguration *config = [WKWebViewConfiguration new];
        [config.userContentController addScriptMessageHandler:gHandler name:@"knit"];
        config.preferences.javaScriptCanOpenWindowsAutomatically = NO;
        gWeb = [[WKWebView alloc] initWithFrame:frame configuration:config];
        gWeb.navigationDelegate = gHandler;
        gWeb.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
        gWeb.allowsMagnification = NO;
        gWeb.allowsBackForwardNavigationGestures = NO;
        // Do not draw a background so the material shows through (no public API; common KVC idiom)
        [gWeb setValue:@NO forKey:@"drawsBackground"];

        KnitDragStrip *strip = [[KnitDragStrip alloc] initWithFrame:NSMakeRect(0, height - 38, width, 38)];
        strip.autoresizingMask = NSViewWidthSizable | NSViewMinYMargin;

        [material addSubview:gWeb];
        [material addSubview:strip];
        gWindow.contentView = material;
        if (![gWindow setFrameUsingName:@"KnitSettingsWeb"]) {
            [gWindow center];
        }
        [gWeb loadHTMLString:[NSString stringWithUTF8String:html] baseURL:nil];
    }
    [NSApp activateIgnoringOtherApps:YES];
    [gWindow deminiaturize:nil];
    [gWindow makeKeyAndOrderFront:nil];
    [gWindow orderFrontRegardless];
    if (@available(macOS 14.0, *)) {
        [NSApp activate];
    }
}

void knit_webui_eval(const char *js) {
    if (gWeb == nil) {
        return;
    }
    [gWeb evaluateJavaScript:[NSString stringWithUTF8String:js] completionHandler:nil];
}

int knit_webui_visible(void) {
    return gWindow != nil && gWindow.isVisible && !gWindow.isMiniaturized;
}

void knit_webui_close(void) {
    [gWindow performClose:nil];
}

// Open/save/folder panel. mode: 0 = open file, 1 = save file, 2 = choose folder.
// Returns NULL when cancelled; free the result with knit_webui_free.
char *knit_webui_choose_path(int mode, const char *name) {
    NSURL *url = nil;
    if (mode == 1) {
        NSSavePanel *panel = [NSSavePanel savePanel];
        panel.nameFieldStringValue = [NSString stringWithUTF8String:name];
        if ([panel runModal] == NSModalResponseOK) {
            url = panel.URL;
        }
    } else {
        NSOpenPanel *panel = [NSOpenPanel openPanel];
        panel.canChooseFiles = mode != 2;
        panel.canChooseDirectories = mode == 2;
        panel.canCreateDirectories = mode == 2;
        panel.allowsMultipleSelection = NO;
        if ([panel runModal] == NSModalResponseOK) {
            url = panel.URL;
        }
    }
    if (url == nil || url.path == nil) {
        return NULL;
    }
    return strdup(url.path.UTF8String);
}

void knit_webui_free(char *p) {
    free(p);
}

void knit_webui_open_url(const char *url) {
    NSURL *u = [NSURL URLWithString:[NSString stringWithUTF8String:url]];
    if (u != nil) {
        [[NSWorkspace sharedWorkspace] openURL:u];
    }
}

void knit_webui_copy(const char *text) {
    NSPasteboard *pb = [NSPasteboard generalPasteboard];
    [pb clearContents];
    [pb setString:[NSString stringWithUTF8String:text] forType:NSPasteboardTypeString];
}

// ---------------------------------------------------------------- shared bridge

typedef void (*KnitMessageFn)(const char *json);

// Script-message bridge that forwards page messages to a Rust function.
@interface KnitBridge : NSObject <WKScriptMessageHandler, WKNavigationDelegate, NSWindowDelegate>
@property(nonatomic, assign) KnitMessageFn onMessage;
@property(nonatomic, assign) BOOL stopsModalOnClose;
@end

@implementation KnitBridge
- (void)userContentController:(WKUserContentController *)controller
      didReceiveScriptMessage:(WKScriptMessage *)message {
    (void)controller;
    if (self.onMessage != NULL && [message.body isKindOfClass:[NSString class]]) {
        self.onMessage([(NSString *)message.body UTF8String]);
    }
}
- (void)webView:(WKWebView *)webView
    decidePolicyForNavigationAction:(WKNavigationAction *)action
                    decisionHandler:(void (^)(WKNavigationActionPolicy))decisionHandler {
    (void)webView;
    if (action.navigationType == WKNavigationTypeLinkActivated && [action.request.URL.scheme isEqualToString:@"https"]) {
        [[NSWorkspace sharedWorkspace] openURL:action.request.URL];
        decisionHandler(WKNavigationActionPolicyCancel);
        return;
    }
    decisionHandler(action.navigationType == WKNavigationTypeOther ? WKNavigationActionPolicyAllow
                                                                    : WKNavigationActionPolicyCancel);
}
- (void)windowWillClose:(NSNotification *)notification {
    (void)notification;
    if (self.stopsModalOnClose && [NSApp modalWindow] != nil) {
        [NSApp stopModalWithCode:0];
    }
}
@end

static WKWebView *knit_make_web(NSRect frame, KnitBridge *bridge, const char *html) {
    WKWebViewConfiguration *config = [WKWebViewConfiguration new];
    [config.userContentController addScriptMessageHandler:bridge name:@"knit"];
    WKWebView *web = [[WKWebView alloc] initWithFrame:frame configuration:config];
    web.navigationDelegate = bridge;
    web.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
    web.allowsMagnification = NO;
    web.allowsBackForwardNavigationGestures = NO;
    [web setValue:@NO forKey:@"drawsBackground"];
    [web loadHTMLString:[NSString stringWithUTF8String:html] baseURL:nil];
    return web;
}

// ---------------------------------------------------------------- setup (modal)

extern void knit_setup_on_message(const char *json);
extern void knit_setup_tick(void);

@interface KnitTicker : NSObject
@end
@implementation KnitTicker
- (void)tick:(NSTimer *)timer {
    (void)timer;
    knit_setup_tick();
}
@end

static WKWebView *gSetupWeb = nil;

// Runs the setup window modally (works before the app's main run loop starts).
// Returns the code passed to knit_setup_stop, or 0 when the window is closed.
long knit_setup_run(const char *html, double width, double height) {
    KnitBridge *bridge = [KnitBridge new];
    bridge.onMessage = knit_setup_on_message;
    bridge.stopsModalOnClose = YES;
    NSRect frame = NSMakeRect(0, 0, width, height);
    NSWindow *win = [[NSWindow alloc] initWithContentRect:frame
                                                styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable |
                                                          NSWindowStyleMaskFullSizeContentView
                                                  backing:NSBackingStoreBuffered
                                                    defer:NO];
    win.releasedWhenClosed = NO;
    win.titlebarAppearsTransparent = YES;
    win.titleVisibility = NSWindowTitleHidden;
    win.title = @"Knit Setup";
    win.delegate = bridge;
    NSVisualEffectView *material = [[NSVisualEffectView alloc] initWithFrame:frame];
    material.material = NSVisualEffectMaterialWindowBackground;
    material.blendingMode = NSVisualEffectBlendingModeBehindWindow;
    material.state = NSVisualEffectStateActive;
    gSetupWeb = knit_make_web(frame, bridge, html);
    KnitDragStrip *strip = [[KnitDragStrip alloc] initWithFrame:NSMakeRect(0, height - 34, width, 34)];
    strip.autoresizingMask = NSViewWidthSizable | NSViewMinYMargin;
    [material addSubview:gSetupWeb];
    [material addSubview:strip];
    win.contentView = material;
    [win center];

    KnitTicker *ticker = [KnitTicker new];
    NSTimer *timer = [NSTimer timerWithTimeInterval:0.25 target:ticker selector:@selector(tick:) userInfo:nil repeats:YES];
    [[NSRunLoop mainRunLoop] addTimer:timer forMode:NSRunLoopCommonModes];

    [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
    [NSApp activateIgnoringOtherApps:YES];
    [win makeKeyAndOrderFront:nil];
    NSModalResponse code = [NSApp runModalForWindow:win];
    [timer invalidate];
    bridge.stopsModalOnClose = NO;
    [win orderOut:nil];
    gSetupWeb = nil;
    [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
    return (long)code;
}

void knit_setup_stop(long code) {
    [NSApp stopModalWithCode:code];
}

void knit_setup_eval(const char *js) {
    if (gSetupWeb != nil) {
        [gSetupWeb evaluateJavaScript:[NSString stringWithUTF8String:js] completionHandler:nil];
    }
}

// ---------------------------------------------------------------- tray popover

extern void knit_tray_on_message(const char *json);
extern void knit_tray_will_show(void);

@interface KnitTray : NSObject <NSPopoverDelegate>
@property(nonatomic, strong) NSStatusItem *item;
@property(nonatomic, strong) NSMenu *menu;
@property(nonatomic, strong) NSPopover *popover;
@property(nonatomic, strong) WKWebView *web;
@property(nonatomic, strong) KnitBridge *bridge;
@end

@implementation KnitTray
- (void)clicked:(id)sender {
    (void)sender;
    NSEvent *event = [NSApp currentEvent];
    BOOL secondary = event.type == NSEventTypeRightMouseUp || (event.modifierFlags & NSEventModifierFlagControl) != 0 ||
                     (event.modifierFlags & NSEventModifierFlagOption) != 0;
    if (secondary) {
        [self showMenu];
    } else if (self.popover.isShown) {
        [self.popover performClose:nil];
    } else {
        knit_tray_will_show();
        // Match the page's light/dark with the popover (the menu bar may be dark in light mode)
        self.popover.appearance = NSApp.effectiveAppearance;
        [NSApp activateIgnoringOtherApps:YES];
        [self.popover showRelativeToRect:self.item.button.bounds ofView:self.item.button preferredEdge:NSRectEdgeMinY];
        [self.popover.contentViewController.view.window makeKeyWindow];
    }
}
- (void)showMenu {
    [self.popover performClose:nil];
    // Attach the classic menu only for this click, then detach so left clicks open the popover
    self.item.menu = self.menu;
    [self.item.button performClick:nil];
    self.item.menu = nil;
}
@end

static KnitTray *gTray = nil;

// Left click opens the popover, right/control/option click opens the classic menu.
void knit_tray_attach(void *item, void *menu, const char *html, double width, double height) {
    gTray = [KnitTray new];
    gTray.item = (__bridge NSStatusItem *)item;
    gTray.menu = (__bridge NSMenu *)menu;
    gTray.item.menu = nil;
    gTray.bridge = [KnitBridge new];
    gTray.bridge.onMessage = knit_tray_on_message;
    NSRect frame = NSMakeRect(0, 0, width, height);
    gTray.web = knit_make_web(frame, gTray.bridge, html);
    NSViewController *vc = [NSViewController new];
    vc.view = gTray.web;
    gTray.popover = [NSPopover new];
    gTray.popover.contentViewController = vc;
    gTray.popover.contentSize = frame.size;
    gTray.popover.behavior = NSPopoverBehaviorTransient;
    gTray.popover.animates = YES;
    NSStatusBarButton *button = gTray.item.button;
    button.target = gTray;
    button.action = @selector(clicked:);
    [button sendActionOn:NSEventMaskLeftMouseUp | NSEventMaskRightMouseUp];
}

void knit_tray_eval(const char *js) {
    if (gTray != nil) {
        [gTray.web evaluateJavaScript:[NSString stringWithUTF8String:js] completionHandler:nil];
    }
}

int knit_tray_shown(void) {
    return gTray != nil && gTray.popover.isShown;
}

void knit_tray_resize(double height) {
    if (gTray != nil && height > 80) {
        gTray.popover.contentSize = NSMakeSize(gTray.popover.contentSize.width, height);
    }
}

void knit_tray_close(void) {
    [gTray.popover performClose:nil];
}

void knit_tray_show_menu(void) {
    [gTray showMenu];
}

// Run `f` on the main queue after the current event (e.g. opening a modal from a script message).
void knit_run_later(void (*f)(void)) {
    dispatch_async(dispatch_get_main_queue(), ^{
        f();
    });
}

// Preview helper: open the popover without a click
void knit_tray_open(void) {
    // Wait until the status item has been drawn, otherwise the popover has nothing to anchor to
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(1.5 * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
        if (gTray != nil && !gTray.popover.isShown) {
            knit_tray_will_show();
            // Preview only: keep it open even though the app is not frontmost
            gTray.popover.behavior = NSPopoverBehaviorApplicationDefined;
            gTray.popover.appearance = NSApp.effectiveAppearance;
            [NSApp activateIgnoringOtherApps:YES];
            [gTray.popover showRelativeToRect:gTray.item.button.bounds ofView:gTray.item.button preferredEdge:NSRectEdgeMinY];
        }
    });
}
