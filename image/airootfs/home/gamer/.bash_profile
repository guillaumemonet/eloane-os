# ~/.bash_profile - Eloane
# Shell de login de l'utilisateur "gamer" (autologin sur tty1 via getty@tty1).

[ -f ~/.bashrc ] && . ~/.bashrc

# Lance la session jeu (gamescope + Steam) uniquement :
#  - sur tty1 (les autres tty restent des consoles normales),
#  - hors session graphique deja active,
#  - si le noyau n'a pas recu "eloane.nosession" (entree de boot de secours),
#  - si ELOANE_NO_SESSION n'est pas defini.
# Pas de "exec" : si eloane-session se termine (quit ou trop de crashs), on
# retombe sur ce shell au lieu de boucler sur l'autologin.
if [ "$(tty)" = /dev/tty1 ] \
    && [ -z "${WAYLAND_DISPLAY:-}" ] && [ -z "${DISPLAY:-}" ] \
    && [ -z "${ELOANE_NO_SESSION:-}" ] \
    && ! grep -qw 'eloane.nosession' /proc/cmdline 2>/dev/null; then
    if command -v eloane-session >/dev/null 2>&1; then
        # Mirror the session output (eloane-session, gamescope, Steam) to the
        # journal so it can be read with: sudo journalctl -b -t eloane-session
        eloane-session > >(tee >(systemd-cat -t eloane-session)) 2>&1
        _eloane_rc=$?
        if [ "$_eloane_rc" -eq 0 ]; then
            echo "eloane-session terminee. Relancer : eloane-session (ou 'logout')."
        else
            echo "eloane-session a echoue (code $_eloane_rc) - shell de secours."
            echo "Journal : sudo journalctl -b -t eloane-session ; relancer : eloane-session"
        fi
        unset _eloane_rc
    else
        echo "eloane-session introuvable (image construite sans --skip-rust ?)."
        echo "Session de test : gamescope -e -f -- steam -gamepadui"
    fi
fi
