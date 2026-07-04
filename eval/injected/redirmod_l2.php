<?php
// Derived from guzzle (src/RedirectMiddleware.php)
// at d281ed313b989f213357e3be1a179f02196ac99b, MIT licensed.
// Planted-clone eval asset for semdup; not production code.
// Same behavior as guzzle's RedirectMiddleware::modifyRequest, restructured:
// each decision is computed into a named value up front and the modification
// array is assembled at the end.

final class RedirectRequestFactory
{
    public function build(RequestInterface $req, array $opts, ResponseInterface $res): RequestInterface
    {
        $status = $res->getStatusCode();
        $downgrade = $status == 303 || ($status <= 302 && !$opts['allow_redirects']['strict']);

        $uri = self::redirectUri($req, $res, $opts['allow_redirects']['protocols']);
        $idn = $opts['idn_conversion'] ?? false;
        if ($idn !== false) {
            $uri = Utils::idnUriConvert($uri, $idn === true ? \IDNA_DEFAULT : $idn);
        }

        $sameScheme = $uri->getScheme() === $req->getUri()->getScheme();
        $sendReferer = $opts['allow_redirects']['referer'] && $sameScheme;
        $crossOrigin = Psr7\UriComparator::isCrossOrigin($req->getUri(), $uri);

        $mod = ['uri' => $uri];
        if ($downgrade) {
            $keep = in_array($req->getMethod(), ['GET', 'HEAD', 'OPTIONS']);
            $mod['method'] = $keep ? $req->getMethod() : 'GET';
            $mod['body'] = '';
        }
        if ($sendReferer) {
            $mod['set_headers']['Referer'] = (string) $req->getUri()->withUserInfo('');
        } else {
            $mod['remove_headers'][] = 'Referer';
        }
        if ($crossOrigin) {
            $mod['remove_headers'][] = 'Authorization';
            $mod['remove_headers'][] = 'Cookie';
        }

        Psr7\Message::rewindBody($req);

        return Psr7\Utils::modifyRequest($req, $mod);
    }
}
