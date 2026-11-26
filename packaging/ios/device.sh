#!/bin/zsh
# Build Quiet.md for an iPhone or iPad connected to this Mac, sign it for
# development, install it and launch it (docs/design/mobile.md). Not a
# release path: no Xcode project, release unless PROFILE=debug.
#
#   packaging/ios/device.sh [device]
#
# The device is a UDID (default: the one paired device that is not a
# simulator). It needs Developer Mode on, and a team provisioning profile
# that lists it: Xcode makes one when signed in to the team's account and
# asked to build for the device once (Xcode › Settings › Accounts, then any
# app run on the device with automatic signing). Console output goes to $LOG
# (default: target/ios-device/console.log).
set -e
cd "$(git rev-parse --show-toplevel)"
bundle_id=com.s1dashu.quietmd
dev=${1:-$(xcrun devicectl list devices 2>/dev/null | grep -v simulated | grep -i paired | grep -oE '[0-9A-F]{8}-[0-9A-F]{16}' | head -1)}
[[ -n $dev ]] || { echo "no paired device: connect it, trust this Mac and turn Developer Mode on" >&2; exit 1 }
profile=${PROFILE:-release}
flag=(); [[ $profile == release ]] && flag=(--release)
out=target/ios-device/$profile
mkdir -p $out

# The newest development profile that lists the device and covers the app.
prov=""
for f in ~/Library/Developer/Xcode/UserData/Provisioning\ Profiles/*.mobileprovision(Nom) ~/Library/MobileDevice/Provisioning\ Profiles/*.mobileprovision(Nom); do
  security cms -D -i "$f" > $out/profile.plist 2>/dev/null || continue
  grep -q "$dev" $out/profile.plist || continue
  app_id=$(/usr/libexec/PlistBuddy -c "Print :Entitlements:application-identifier" $out/profile.plist 2>/dev/null)
  [[ $app_id == *.$bundle_id || $app_id == *.\* ]] && { prov=$f; break }
done
[[ -n $prov ]] || { echo "no provisioning profile lists $dev: build any app for it once from Xcode with the team's account" >&2; exit 1 }
team=$(/usr/libexec/PlistBuddy -c "Print :Entitlements:com.apple.developer.team-identifier" $out/profile.plist)
# The team's development identity (its certificate's OU is the team).
ident=""
for h in ${(f)"$(security find-identity -v -p codesigning | grep 'Apple Development' | awk '{print $2}')"}; do
  security find-certificate -a -Z -p -c "Apple Development" | awk -v h=$h '/SHA-1 hash:/ {keep = ($3 == h)} keep' | sed -n '/BEGIN/,/END/p' | openssl x509 -noout -subject 2>/dev/null | grep -q "OU *= *$team" && { ident=$h; break }
done
[[ -n $ident ]] || { echo "no Apple Development identity for team $team in the keychain" >&2; exit 1 }

# Only the app: the workspace's quiet CLI embeds QuickJS, which has no iOS bindings.
IPHONEOS_DEPLOYMENT_TARGET=17.0 cargo build -p quiet_md --target aarch64-apple-ios $flag
app=$out/Quiet.md.app
rm -rf $app && mkdir -p $app
cp target/aarch64-apple-ios/$profile/quiet_md $app/quiet_md
cp packaging/ios/Info.plist $app/
cp "$prov" $app/embedded.mobileprovision
cat > $out/entitlements.plist <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>application-identifier</key><string>$team.$bundle_id</string>
  <key>com.apple.developer.team-identifier</key><string>$team</string>
  <key>get-task-allow</key><true/>
  <key>keychain-access-groups</key><array><string>$team.$bundle_id</string></array>
</dict></plist>
EOF
codesign --force --sign $ident --entitlements $out/entitlements.plist $app
xcrun devicectl device install app --device $dev $app
log=${LOG:-target/ios-device/console.log}
xcrun devicectl device process launch --device $dev --terminate-existing --console $bundle_id > $log 2>&1 &
echo "launched on $dev; console in $log"
