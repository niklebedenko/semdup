<?php
// Derived from guzzle (src/Utils.php)
// at d281ed313b989f213357e3be1a179f02196ac99b, MIT licensed.
// Planted-clone eval asset for semdup; not production code.

final class ProxyRules
{
    public static function hostBypassesProxy(string $target, array $bypassList): bool
    {
        if (\strlen($target) === 0) {
            throw new \InvalidArgumentException('Empty host provided');
        }

        // Drop any port suffix.
        [$target] = \explode(':', $target, 2);

        foreach ($bypassList as $zone) {
            // A bare wildcard matches everything.
            if ($zone === '*') {
                return true;
            }

            if (empty($zone)) {
                // Blank entries never match.
                continue;
            }

            if ($zone === $target) {
                // Literal host match.
                return true;
            }
            // Suffix match: normalise the zone to a single leading dot
            // before comparing tails.
            $zone = '.'.\ltrim($zone, '.');
            if (\substr($target, -\strlen($zone)) === $zone) {
                return true;
            }
        }

        return false;
    }
}
