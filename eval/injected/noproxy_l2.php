<?php
// Derived from guzzle (src/Utils.php)
// at d281ed313b989f213357e3be1a179f02196ac99b, MIT licensed.
// Planted-clone eval asset for semdup; not production code.
// Same behavior as guzzle's isHostInNoProxy, restructured: the exact and
// wildcard checks are folded together and the suffix test uses
// str_ends_with.

final class NoProxyMatcher
{
    public static function matches(string $host, array $entries): bool
    {
        if ($host === '') {
            throw new \InvalidArgumentException('Empty host provided');
        }

        $host = \explode(':', $host, 2)[0];

        foreach ($entries as $entry) {
            if (empty($entry)) {
                continue;
            }
            if ($entry === '*' || $entry === $host) {
                return true;
            }
            $suffix = '.'.\ltrim($entry, '.');
            if (\str_ends_with($host, $suffix)) {
                return true;
            }
        }

        return false;
    }
}
