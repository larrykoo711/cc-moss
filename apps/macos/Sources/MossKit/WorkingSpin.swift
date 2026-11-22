import Foundation

/// When the "working" ring turns. Hiding the panel only orders its window
/// out: the view stays alive, and a ring that kept turning there cost the app
/// a third of a core while nobody could see it.
public enum WorkingSpin {
    public static func runs(panelShown: Bool, reduceMotion: Bool) -> Bool {
        panelShown && !reduceMotion
    }
}
