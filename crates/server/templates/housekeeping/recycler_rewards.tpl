{% set title = locale.hk_reciclador_ecotron %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="toolbar">
  <span class="muted">{{ locale.hk_premios_que_puede_dar_el_reciclador_cada_premio|raw }}</span>
  <span class="spacer"></span>
  <a class="btn primary" href="{{ hk.path }}/recycler_rewards/edit">{{ locale.hk_nuevo_premio|raw }}</a>
</div>

<div class="card flush">
  <div class="card-body scroll">
    {% if rewards is empty %}<div class="empty">{{ locale.hk_no_hay_premios_configurados|raw }}</div>{% else %}
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_sprite|raw }}</th><th class="num">{{ locale.hk_orden|raw }}</th><th class="num">{{ locale.hk_nivel|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for reward in rewards %}
        <tr>
          <td class="mono"><a href="{{ hk.path }}/recycler_rewards/edit?sprite={{ reward.sprite() }}">{{ reward.sprite() }}</a></td>
          <td class="num">{{ reward.orderId() }}</td>
          <td class="num">{{ locale.hk_1_entre|raw }} {{ reward.chance() }}{% if reward.chance() != 5 and reward.chance() != 40 and reward.chance() != 200 and reward.chance() != 2000 %} <span class="pill warn" title="{{ locale.hk_nivel_no_reconocido_por_el_reciclador|raw }}">?</span>{% endif %}</td>
          <td class="actions">
            <a class="btn sm" href="{{ hk.path }}/recycler_rewards/edit?sprite={{ reward.sprite() }}">{{ locale.hk_editar|raw }}</a>
            <form class="inline" method="post" action="{{ hk.path }}/recycler_rewards/delete?sprite={{ reward.sprite() }}" data-confirm="{{ locale.hk_quitar_2|raw }} {{ reward.sprite() }} {{ locale.hk_de_los_premios_del_reciclador|raw }}">
              <input type="hidden" name="hk_token" value="{{ hk.token }}">
              <button class="btn sm ghost-danger" type="submit">{{ locale.hk_borrar|raw }}</button>
            </form>
          </td>
        </tr>
      {% endfor %}
      </tbody>
    </table>
    {% endif %}
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
